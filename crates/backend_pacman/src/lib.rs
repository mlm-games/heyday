use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader, Read},
    path::Path,
    process::{Command, Stdio},
    str::FromStr,
    sync::Arc,
};

use alpm_db::desc::{DbDescFileV1, DbDescFileV2};
use alpm_repo_db::desc::RepoDescFileV2;
use flate2::read::GzDecoder;
use parking_lot::Mutex;
use tar::Archive;

use domain::*;

const PACMAN_DB: &str = "/var/lib/pacman";
const PACMAN_CACHE: &str = "/var/cache/pacman/pkg";
const HELPER_BIN: &str = "pacman-helper";

fn parse_json_stage(s: Option<&str>) -> Stage {
    match s {
        Some("refreshing") => Stage::Refreshing,
        Some("downloading") => Stage::Downloading,
        Some("installing") => Stage::Installing,
        Some("removing") => Stage::Removing,
        Some("resolving") => Stage::Resolving,
        Some("verifying") => Stage::Verifying,
        Some("cleaning") => Stage::Cleaning,
        Some("keyring") => Stage::Verifying,
        _ => Stage::Installing,
    }
}

pub struct PacmanCli;

fn parse_size(s: &str) -> Option<u64> {
    // Parse pacman size strings like "12.34 MiB", "5.67 KiB", "1234.00  B"
    let s = s.trim();
    if s.is_empty() || s == "None" {
        return None;
    }
    let (num_str, unit) = s.trim().split_once(' ').unwrap_or((s, ""));
    let val: f64 = num_str.parse().ok()?;
    let bytes = match unit.trim() {
        "KiB" => val * 1024.0,
        "MiB" => val * 1024.0 * 1024.0,
        "GiB" => val * 1024.0 * 1024.0 * 1024.0,
        "B" | "" => val,
        _ => val,
    };
    Some(bytes as u64)
}

impl Default for PacmanCli {
    fn default() -> Self {
        Self::new()
    }
}

impl PacmanCli {
    pub fn new() -> Self {
        Self
    }

    fn read_local_db() -> HashMap<String, DbDescFileV1> {
        let local_dir = Path::new(PACMAN_DB).join("local");
        let mut packages = HashMap::new();
        let dir = match fs::read_dir(&local_dir) {
            Ok(d) => d,
            Err(_) => return packages,
        };
        for entry in dir.flatten() {
            let pkg_dir = entry.path();
            if !pkg_dir.is_dir() {
                continue;
            }
            let desc_path = pkg_dir.join("desc");
            let content = match fs::read_to_string(&desc_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            // Try V2 first (with %XDATA%), fall back to V1
            if let Ok(desc) = DbDescFileV2::from_str(&content) {
                packages.insert(desc.name.to_string(), DbDescFileV1 {
                    name: desc.name,
                    version: desc.version,
                    base: desc.base,
                    description: desc.description,
                    url: desc.url,
                    arch: desc.arch,
                    builddate: desc.builddate,
                    installdate: desc.installdate,
                    packager: desc.packager,
                    size: desc.size,
                    groups: desc.groups,
                    reason: desc.reason,
                    license: desc.license,
                    validation: desc.validation,
                    replaces: desc.replaces,
                    depends: desc.depends,
                    optdepends: desc.optdepends,
                    conflicts: desc.conflicts,
                    provides: desc.provides,
                });
            } else if let Ok(desc) = DbDescFileV1::from_str(&content) {
                packages.insert(desc.name.to_string(), desc);
            }
        }
        packages
    }

    fn read_local_db_names() -> HashSet<String> {
        // pacman -Qq is the authoritative source of installed package names.
        // It always works regardless of desc file format issues.
        let out = Command::new("pacman")
            .args(["-Qq"])
            .output()
            .ok();
        let mut names = HashSet::new();
        if let Some(out) = out {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let n = line.trim();
                if !n.is_empty() {
                    names.insert(n.to_string());
                }
            }
        }
        names
    }

    fn read_sync_db(repo_path: &Path) -> HashMap<String, RepoDescFileV2> {
        let file = match fs::File::open(repo_path) {
            Ok(f) => f,
            Err(_) => return HashMap::new(),
        };
        let decoder = GzDecoder::new(file);
        let mut archive = Archive::new(decoder);
        let mut packages = HashMap::new();

        let entries = match archive.entries() {
            Ok(e) => e,
            Err(_) => return packages,
        };

        for mut entry in entries.flatten() {
            let path = match entry.path() {
                Ok(p) => p,
                Err(_) => continue,
            };
            if !path.ends_with("desc") {
                continue;
            }
            let mut content = String::new();
            if entry.read_to_string(&mut content).is_err() {
                continue;
            }
            if let Ok(desc) = RepoDescFileV2::from_str(&content) {
                packages.insert(desc.name.to_string(), desc);
            }
        }
        packages
    }

    fn read_all_sync_dbs() -> HashMap<String, HashMap<String, RepoDescFileV2>> {
        let sync_dir = Path::new(PACMAN_DB).join("sync");
        let mut repos = HashMap::new();
        let dir = match fs::read_dir(&sync_dir) {
            Ok(d) => d,
            Err(_) => return repos,
        };
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("db") {
                continue;
            }
            let repo_name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let packages = Self::read_sync_db(&path);
            repos.insert(repo_name, packages);
        }
        repos
    }

    fn installed_names() -> HashSet<String> {
        // Primary: parse directory names from /var/lib/pacman/local/<name>-<version>/
        // This is more robust than parsing desc files which may fail on format issues.
        let mut names = Self::read_local_db_names();
        // Secondary: also include any names from desc file parsing
        // (covers edge cases where dir name differs from internal name)
        names.extend(Self::read_local_db().into_keys());
        names
    }

    fn run_stream(
        &self,
        mut cmd: Command,
        sink: &ProgressSink,
        cancel: &CancelToken,
        stage: Stage,
    ) -> Result<(i32, Option<String>)> {
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Internal(format!("spawn: {e}")))?;
        let out = child.stdout.take().expect("stdout should be piped");
        let err = child.stderr.take().expect("stderr should be piped");

        let jid = 0u64;
        let tx1 = sink.clone();
        let tx2 = sink.clone();

        let stage_out = stage;
        let stage_err = stage;

        let last_err = Arc::new(Mutex::new(None::<String>));
        let last_err_t2 = last_err.clone();

        let t1 = std::thread::spawn(move || {
            #[allow(clippy::lines_filter_map_ok)]
            for l in BufReader::new(out).lines().filter_map(|r| r.ok()) {
                let _ = tx1.send(Progress {
                    job_id: jid,
                    stage: stage_out,
                    percent: None,
                    bytes: None,
                    log: Some(l),
                    warning: false,
                });
            }
        });

        let t2 = std::thread::spawn(move || {
            #[allow(clippy::lines_filter_map_ok)]
            for l in BufReader::new(err).lines().filter_map(|r| r.ok()) {
                {
                    let mut g = last_err_t2.lock();
                    *g = Some(l.clone());
                }
                let _ = tx2.send(Progress {
                    job_id: jid,
                    stage: stage_err,
                    percent: None,
                    bytes: None,
                    log: Some(l),
                    warning: true,
                });
            }
        });

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let _ = t1.join();
                    let _ = t2.join();
                    let code = status.code().unwrap_or(-1);
                    let last = last_err.lock().clone();
                    return Ok((code, last));
                }
                Ok(None) => {
                    if cancel.is_cancelled() {
                        #[cfg(unix)]
                        {
                            let _ = nix::sys::signal::kill(
                                nix::unistd::Pid::from_raw(child.id() as i32),
                                nix::sys::signal::Signal::SIGTERM,
                            );
                        }
                        let _ = child.wait();
                        let _ = t1.join();
                        let _ = t2.join();
                        return Err(Error::Cancelled);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(16));
                }
                Err(e) => return Err(Error::Internal(format!("wait: {e}"))),
            }
        }
    }

    fn try_run_helper(
        &self,
        helper_args: &[&str],
        stage: Stage,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> std::result::Result<(), Option<Error>> {
        let mut cmd = Command::new("pkexec");
        cmd.arg(HELPER_BIN);
        cmd.args(helper_args);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return Err(Some(Error::Internal(format!("spawn helper: {e}")))),
        };
        let out = child.stdout.take().expect("stdout piped");
        let err = child.stderr.take().expect("stderr piped");

        let jid = 0u64;

        // Stderr → warning log lines
        let tx_err = sink.clone();
        let cancel_err = cancel.clone();
        let err_thread = std::thread::spawn(move || {
            for l in BufReader::new(err).lines().filter_map(|r| r.ok()) {
                if cancel_err.is_cancelled() {
                    break;
                }
                let _ = tx_err.send(Progress {
                    job_id: jid,
                    stage,
                    percent: None,
                    bytes: None,
                    log: Some(l),
                    warning: true,
                });
            }
        });

        // Stdout → JSON progress lines
        for line in BufReader::new(out).lines() {
            let line = match line {
                Ok(l) if l.is_empty() => continue,
                Ok(l) => l,
                Err(_) => break,
            };

            if cancel.is_cancelled() {
                #[cfg(unix)]
                {
                    let _ = nix::sys::signal::kill(
                        nix::unistd::Pid::from_raw(child.id() as i32),
                        nix::sys::signal::Signal::SIGTERM,
                    );
                }
                let _ = child.wait();
                err_thread.join().ok();
                return Err(None);
            }

            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                match val["type"].as_str() {
                    Some("progress") => {
                        let parsed_stage = parse_json_stage(val["stage"].as_str());
                        let percent = val["percent"].as_f64().map(|p| p as f32);
                        let log = val["log"].as_str().map(|s| s.to_string());
                        let warning = val["warning"].as_bool().unwrap_or(false);
                        let bytes = val["bytes"].as_array().and_then(|a| {
                            if a.len() == 2 {
                                Some((
                                    a[0].as_u64().unwrap_or(0),
                                    a[1].as_u64().unwrap_or(0),
                                ))
                            } else {
                                None
                            }
                        });
                        let _ = sink.send(Progress {
                            job_id: jid,
                            stage: parsed_stage,
                            percent,
                            bytes,
                            log,
                            warning,
                        });
                    }
                    Some("result") => {
                        let code = val["code"].as_i64().unwrap_or(1);
                        err_thread.join().ok();
                        let _ = child.wait();
                        if code == 0 {
                            return Ok(());
                        } else {
                            let msg = val["message"]
                                .as_str()
                                .unwrap_or("helper failed")
                                .to_string();
                            return Err(Some(Error::Priv(msg)));
                        }
                    }
                    _ => {
                        let _ = sink.send(Progress {
                            job_id: jid,
                            stage,
                            percent: None,
                            bytes: None,
                            log: Some(line),
                            warning: true,
                        });
                    }
                }
            } else {
                let _ = sink.send(Progress {
                    job_id: jid,
                    stage,
                    percent: None,
                    bytes: None,
                    log: Some(line),
                    warning: true,
                });
            }
        }

        err_thread.join().ok();
        let status = child.wait().unwrap_or_default();
        let code = status.code().unwrap_or(-1);
        if code == 0 {
            Ok(())
        } else {
            Err(Some(Error::Priv(format!("helper exit {code}"))))
        }
    }

    fn fallback_pacman(
        &self,
        args: &[&str],
        stage: Stage,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> Result<()> {
        let mut cmd = Command::new("pkexec");
        cmd.args(["pacman"]);
        cmd.args(args);
        let (code, last_err) = self.run_stream(cmd, sink, cancel, stage)?;
        if code == 0 {
            Ok(())
        } else {
            let why = last_err.map(|e| format!(": {e}")).unwrap_or_default();
            Err(Error::Priv(format!("pacman exit {code}{why}")))
        }
    }
}

impl PacmanCli {
    pub fn install_local(
        &self,
        path: &str,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> Result<()> {
        let _ = sink.send(Progress {
            job_id: 0,
            stage: Stage::Installing,
            percent: None,
            bytes: None,
            log: Some(format!("installing from local file: {path}")),
            warning: false,
        });
        self.fallback_pacman(&["-U", "--noconfirm", path], Stage::Installing, sink, cancel)
    }

    pub fn cached_versions(name: &str) -> Vec<(String, String)> {
        let dir = Path::new(PACMAN_CACHE);
        let mut versions = Vec::new();
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return versions,
        };
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.starts_with(&format!("{name}-")) && fname.ends_with(".pkg.tar.zst") {
                let ver = fname
                    .strip_prefix(&format!("{name}-"))
                    .and_then(|s| s.rsplit_once('-'))
                    .map(|(v, _)| v.to_string())
                    .unwrap_or_default();
                if !ver.is_empty() {
                    versions.push((fname, ver));
                }
            }
        }
        versions.sort_by(|a, b| b.1.cmp(&a.1)); // newest first
        versions
    }

    pub fn cache_clean(
        &self,
        keep: u32,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> Result<()> {
        let _ = sink.send(Progress {
            job_id: 0,
            stage: Stage::Cleaning,
            percent: None,
            bytes: None,
            log: Some(format!("cleaning cache, keeping {keep} versions")),
            warning: false,
        });
        let r = self.try_run_helper(&["cache-clean", &keep.to_string()], Stage::Cleaning, sink, cancel);
        match r {
            Ok(()) => Ok(()),
            Err(Some(e)) => {
                let _ = sink.send(Progress {
                    job_id: 0,
                    stage: Stage::Cleaning,
                    percent: None,
                    bytes: None,
                    log: Some(format!("helper unavailable ({e}); fallback not implemented")),
                    warning: true,
                });
                Err(Error::Alpm(format!("cache clean failed: {e}")))
            }
            Err(None) => Err(Error::Cancelled),
        }
    }

    pub fn install_downgrade(
        &self,
        name: &str,
        version: &str,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> Result<()> {
        let path = format!("{PACMAN_CACHE}/{name}-{version}-{}.pkg.tar.zst", std::env::consts::ARCH);
        if !Path::new(&path).exists() {
            let path_any = format!("{PACMAN_CACHE}/{name}-{version}-any.pkg.tar.zst");
            if Path::new(&path_any).exists() {
                return self.install_local(&path_any, sink, cancel);
            }
            return Err(Error::Alpm(format!("cached package not found: {path}")));
        }
        self.install_local(&path, sink, cancel)
    }
}

impl PacmanCli {
    pub fn available_groups() -> Vec<String> {
        let sync_dbs = Self::read_all_sync_dbs();
        let mut groups: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for packages in sync_dbs.values() {
            for pkg in packages.values() {
                for g in &pkg.groups {
                    groups.insert(g.to_string());
                }
            }
        }
        groups.into_iter().collect()
    }

    pub fn orphans_list() -> Result<Vec<PackageSummary>> {
        let out = Command::new("pacman")
            .args(["-Qdtq"])
            .output()
            .map_err(|e| Error::Internal(format!("pacman -Qdtq: {e}")))?;
        if !out.status.success() {
            return Ok(vec![]);
        }
        let local = Self::read_local_db();
        let sync_dbs = Self::read_all_sync_dbs();
        let names: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        let mut results = Vec::new();
        for name in names {
            let installed = local.contains_key(&name);
            let mut summary = PackageSummary {
                id: PackageId { name: name.clone(), source: Source::Repo },
                version: String::new(),
                description: String::new(),
                groups: vec![],
                installed,
                popular: None,
                last_updated: None,
            };
            // Try to get details from local DB
            if let Some(lpkg) = local.get(&name) {
                summary.version = lpkg.version.to_string();
                summary.description = lpkg.description.to_string();
            }
            // Try sync DB for version/description
            for packages in sync_dbs.values() {
                if let Some(spkg) = packages.get(&name) {
                    summary.version = spkg.version.to_string();
                    summary.description = spkg.description.to_string();
                    summary.groups = spkg.groups.iter().map(|g| g.to_string()).collect();
                    break;
                }
            }
            results.push(summary);
        }
        results.sort_by(|a, b| a.id.name.cmp(&b.id.name));
        Ok(results)
    }

    pub fn list_installed() -> Result<Vec<PackageSummary>> {
        let local_by_name = Self::read_local_db();
        let sync_dbs = Self::read_all_sync_dbs();
        let mut results = Vec::new();

        // Use pacman -Qq as authoritative source of installed names
        let out = Command::new("pacman")
            .args(["-Qq"])
            .output()
            .map_err(|e| Error::Internal(format!("pacman -Qq: {e}")))?;
        if !out.status.success() {
            return Err(Error::Alpm("pacman -Qq failed".into()));
        }

        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let name = line.trim().to_string();
            if name.is_empty() {
                continue;
            }

            let version = local_by_name.get(&name)
                .map(|p| p.version.to_string())
                .unwrap_or_default();

            let description = local_by_name.get(&name)
                .map(|p| p.description.to_string())
                .unwrap_or_default();

            let mut groups: Vec<String> = Vec::new();
            for packages in sync_dbs.values() {
                if let Some(spkg) = packages.get(&name) {
                    if spkg.version.to_string() == version {
                        groups = spkg.groups.iter().map(|g| g.to_string()).collect();
                    }
                    break;
                }
            }

            results.push(PackageSummary {
                id: PackageId { name, source: Source::Repo },
                version,
                description,
                groups,
                installed: true,
                popular: None,
                last_updated: None,
            });
        }

        results.sort_by(|a, b| a.id.name.cmp(&b.id.name));
        Ok(results)
    }

    pub fn export_list() -> Result<String> {
        let out = Command::new("pacman")
            .args(["-Qqe"])
            .output()
            .map_err(|e| Error::Internal(format!("pacman -Qqe: {e}")))?;
        if !out.status.success() {
            return Err(Error::Alpm("pacman -Qqe failed".into()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    pub fn pacnew_list() -> Result<Vec<PacnewFile>> {
        let mut files = Vec::new();
        let find = Command::new("find")
            .args(["/etc", "-name", "*.pacnew", "-maxdepth", "2"])
            .output()
            .map_err(|e| Error::Internal(format!("find pacnew: {e}")))?;
        for line in String::from_utf8_lossy(&find.stdout).lines() {
            let path = line.trim().to_string();
            if path.is_empty() { continue; }
            let pkg = Self::pkg_for_path(&path);
            files.push(PacnewFile { path, package: pkg });
        }
        // Also check /var and /usr
        for base in &["/var", "/usr"] {
            let find = Command::new("find")
                .args([base, "-name", "*.pacnew", "-maxdepth", "3"])
                .output()
                .map_err(|e| Error::Internal(format!("find pacnew in {base}: {e}")))?;
            for line in String::from_utf8_lossy(&find.stdout).lines() {
                let path = line.trim().to_string();
                if path.is_empty() { continue; }
                let pkg = Self::pkg_for_path(&path);
                files.push(PacnewFile { path, package: pkg });
            }
        }
        Ok(files)
    }

    fn pkg_for_path(path: &str) -> String {
        // Use pacman -Qo to find owner
        let out = Command::new("pacman")
            .args(["-Qo", path])
            .output()
            .ok();
        if let Some(out) = out {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout);
                if let Some(line) = s.lines().next() {
                    if let Some(name) = line.split_whitespace().next() {
                        return name.to_string();
                    }
                }
            }
        }
        "unknown".into()
    }

    pub fn verify_packages() -> Result<String> {
        let out = Command::new("pacman")
            .args(["-Qkk", "--quiet"])
            .output()
            .map_err(|e| Error::Internal(format!("pacman -Qkk: {e}")))?;
        if out.status.success() {
            Ok("All packages verified OK.".into())
        } else {
            let output = String::from_utf8_lossy(&out.stdout).to_string();
            let lines: Vec<&str> = output.lines()
                .filter(|l| !l.is_empty())
                .collect();
            if lines.is_empty() {
                Ok("All packages verified OK.".into())
            } else {
                Ok(lines.join("\n"))
            }
        }
    }
}

impl PackageBackend for PacmanCli {
    fn install(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let r = self.try_run_helper(
            &["install", &id.name],
            Stage::Installing,
            sink,
            cancel,
        );
        match r {
            Ok(()) => Ok(()),
            Err(Some(e)) => {
                // Helper unavailable — fall back to CLI
                let _ = sink.send(Progress {
                    job_id: 0,
                    stage: Stage::Installing,
                    percent: None,
                    bytes: None,
                    log: Some(format!("helper unavailable ({e}); falling back to pacman CLI")),
                    warning: true,
                });
                self.fallback_pacman(
                    &["-S", "--noconfirm", "--needed", &id.name],
                    Stage::Installing,
                    sink,
                    cancel,
                )
            }
            Err(None) => Err(Error::Cancelled),
        }
    }

    fn refresh(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        // First try the helper
        match self.try_run_helper(&["refresh"], Stage::Refreshing, sink, cancel) {
            Ok(()) => return Ok(()),
            Err(Some(_)) => {}
            Err(None) => return Err(Error::Cancelled),
        }

        // Fallback: pkexec pacman -Sy
        let try_pkexec = || -> Result<()> {
            let mut cmd = Command::new("pkexec");
            cmd.args(["pacman", "-Sy", "--noconfirm"]);
            let (code, last_err) = self.run_stream(cmd, sink, cancel, Stage::Refreshing)?;
            if code == 0 {
                Ok(())
            } else {
                let why = last_err.map(|e| format!(": {e}")).unwrap_or_default();
                Err(Error::Alpm(format!("pacman -Sy exit {code}{why}")))
            }
        };

        match try_pkexec() {
            Ok(()) => Ok(()),
            Err(Error::Internal(e)) if e.contains("spawn:") => {
                let _ = sink.send(Progress {
                    job_id: 0,
                    stage: Stage::Refreshing,
                    percent: None,
                    bytes: None,
                    log: Some(
                        "pkexec unavailable; attempting unprivileged refresh (may fail)".into(),
                    ),
                    warning: true,
                });
                let mut cmd = Command::new("pacman");
                cmd.args(["-Sy", "--noconfirm"]);
                let (code, last_err) = self.run_stream(cmd, sink, cancel, Stage::Refreshing)?;
                if code == 0 {
                    Ok(())
                } else {
                    let why = last_err.map(|e| format!(": {e}")).unwrap_or_default();
                    Err(Error::Alpm(format!("pacman -Sy exit {code}{why}")))
                }
            }
            Err(e) => Err(e),
        }
    }

    fn search(
        &self,
        q: &str,
        sink: &ProgressSink,
        _cancel: &CancelToken,
    ) -> Result<Vec<PackageSummary>> {
        let q = q.trim();
        if q.len() < 2 {
            sink.send(Progress {
                job_id: 0,
                stage: Stage::Searching,
                percent: None,
                bytes: None,
                log: Some("repo: query too short (<2), ignoring".into()),
                warning: true,
            })
            .ok();
            return Ok(vec![]);
        }

        sink.send(Progress {
            job_id: 0,
            stage: Stage::Searching,
            percent: None,
            bytes: None,
            log: Some(format!("repo search: {q}")),
            warning: false,
        })
        .ok();

        let installed = Self::installed_names();
        let sync_dbs = Self::read_all_sync_dbs();
        let q_lower = q.to_lowercase();

        // Use pacman -Ssq as authoritative source (never drops packages due to desc parsing issues)
        let out = match Command::new("pacman").args(["-Ssq", q]).output() {
            Ok(o) => o,
            Err(e) => {
                sink.send(Progress {
                    job_id: 0,
                    stage: Stage::Searching,
                    percent: None,
                    bytes: None,
                    log: Some(format!("repo: pacman -Ssq failed: {e}"),),
                    warning: true,
                })
                .ok();
                return Ok(vec![]);
            }
        };

        let mut results: Vec<PackageSummary> = Vec::new();

        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let name = line.trim().to_string();
            if name.is_empty() {
                continue;
            }

            let mut version = String::new();
            let mut description = String::new();
            let mut groups: Vec<String> = Vec::new();
            // Enrich with sync DB data
            for packages in sync_dbs.values() {
                if let Some(spkg) = packages.get(&name) {
                    version = spkg.version.to_string();
                    description = spkg.description.to_string();
                    groups = spkg.groups.iter().map(|g| g.to_string()).collect();
                    break;
                }
            }

            results.push(PackageSummary {
                id: PackageId { name, source: Source::Repo },
                version,
                description,
                groups,
                installed: installed.contains(line.trim()),
                popular: None,
                last_updated: None,
            });
        }

        // Also search descriptions from sync DBs for packages not found by -Ssq
        if results.is_empty() {
            for packages in sync_dbs.values() {
                for pkg in packages.values() {
                    let name = pkg.name.to_string();
                    let desc_lower = pkg.description.to_string().to_lowercase();
                    if desc_lower.contains(&q_lower)
                        && !results.iter().any(|r| r.id.name == name)
                    {
                        results.push(PackageSummary {
                            id: PackageId { name, source: Source::Repo },
                            version: pkg.version.to_string(),
                            description: pkg.description.to_string(),
                            groups: pkg.groups.iter().map(|g| g.to_string()).collect(),
                            installed: installed.contains(pkg.name.to_string().as_str()),
                            popular: None,
                            last_updated: None,
                        });
                    }
                }
            }
        }

        results.sort_by(|a, b| a.id.name.cmp(&b.id.name));
        results.truncate(500);

        if results.is_empty() {
            sink.send(Progress {
                job_id: 0,
                stage: Stage::Searching,
                percent: None,
                bytes: None,
                log: Some("repo: search returned 0 matches".into()),
                warning: false,
            })
            .ok();
        } else {
            sink.send(Progress {
                job_id: 0,
                stage: Stage::Searching,
                percent: None,
                bytes: None,
                log: Some(format!("repo: search yielded {} matches", results.len())),
                warning: false,
            })
            .ok();
        }

        Ok(results)
    }

    fn details(
        &self,
        id: &PackageId,
        _sink: &ProgressSink,
        _cancel: &CancelToken,
    ) -> Result<PackageDetails> {
        // Try pacman -Si first (repos), fall back to -Qi (installed-only)
        let out = Command::new("pacman")
            .args(["-Si", &id.name])
            .output()
            .and_then(|o| if o.status.success() { Ok(o) } else {
                Command::new("pacman").args(["-Qi", &id.name]).output()
            })
            .map_err(|e| Error::Internal(format!("pacman -Si/-Qi: {e}")))?;

        if !out.status.success() {
            return Err(Error::Alpm(format!(
                "package {} not found", id.name
            )));
        }

        let s = String::from_utf8_lossy(&out.stdout);
        let installed = Self::installed_names().contains(&id.name);

        // Extract fields from pacman -Si/-Qi output
        let mut version = String::new();
        let mut description = String::new();
        let mut groups: Vec<String> = Vec::new();
        let mut depends: Vec<String> = Vec::new();
        let mut opt_depends: Vec<String> = Vec::new();
        let mut homepage = None;
        let mut size_install = None;
        let mut size_download = None;
        let mut maintainer = None;

        let mut in_dep = false;
        let mut in_opt = false;

        for raw in s.lines() {
            let line = raw.trim_end();
            if let Some(v) = line.strip_prefix("Version         :") {
                version = v.trim().to_string();
                in_dep = false; in_opt = false;
            } else if let Some(v) = line.strip_prefix("Description     :") {
                description = v.trim().to_string();
                in_dep = false; in_opt = false;
            } else if let Some(v) = line.strip_prefix("Groups          :") {
                let v = v.trim();
                if v != "None" {
                    groups = v.split_whitespace().map(|s| s.to_string()).collect();
                }
                in_dep = false; in_opt = false;
            } else if let Some(v) = line.strip_prefix("URL             :") {
                let v = v.trim();
                if !v.is_empty() && v != "None" {
                    homepage = Some(v.to_string());
                }
                in_dep = false; in_opt = false;
            } else if let Some(v) = line.strip_prefix("Licenses        :") {
                // skip
                in_dep = false; in_opt = false;
            } else if let Some(v) = line.strip_prefix("Depends On      :") {
                in_dep = true; in_opt = false;
                let v = v.trim();
                if v != "None" {
                    for tok in v.split_whitespace() {
                        let n = tok.split(|c| c == '<' || c == '>' || c == '=').next().unwrap_or("").trim();
                        if !n.is_empty() { depends.push(n.to_string()); }
                    }
                }
            } else if let Some(v) = line.strip_prefix("Optional Deps   :") {
                in_dep = false; in_opt = true;
                let v = v.trim();
                if !v.is_empty() {
                    if let Some(name) = v.split(':').next() {
                        let n = name.trim();
                        if !n.is_empty() { opt_depends.push(n.to_string()); }
                    }
                }
            } else if let Some(v) = line.strip_prefix("Required By     :") {
                in_dep = false; in_opt = false;
            } else if let Some(v) = line.strip_prefix("Optional For    :") {
                in_dep = false; in_opt = false;
            } else if let Some(v) = line.strip_prefix("Installed Size  :") {
                in_dep = false; in_opt = false;
                size_install = parse_size(v);
            } else if let Some(v) = line.strip_prefix("Download Size   :") {
                in_dep = false; in_opt = false;
                size_download = parse_size(v);
            } else if let Some(v) = line.strip_prefix("Packager        :") {
                let v = v.trim();
                if !v.is_empty() && v != "None" {
                    maintainer = Some(v.to_string());
                }
                in_dep = false; in_opt = false;
            } else if line.starts_with(' ') || line.starts_with('\t') {
                if in_dep {
                    for tok in line.split_whitespace() {
                        let n = tok.split(|c| c == '<' || c == '>' || c == '=').next().unwrap_or("").trim();
                        if !n.is_empty() { depends.push(n.to_string()); }
                    }
                } else if in_opt {
                    if let Some(name) = line.split(':').next() {
                        let n = name.trim();
                        if !n.is_empty() { opt_depends.push(n.to_string()); }
                    }
                }
            } else {
                in_dep = false; in_opt = false;
            }
        }

        Ok(PackageDetails {
            summary: PackageSummary {
                id: id.clone(),
                version,
                description,
                groups,
                installed,
                popular: None,
                last_updated: None,
            },
            depends,
            opt_depends,
            homepage,
            maintainer,
            size_install,
            size_download,
        })
    }

    fn remove(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let r = self.try_run_helper(
            &["remove", &id.name],
            Stage::Removing,
            sink,
            cancel,
        );
        match r {
            Ok(()) => Ok(()),
            Err(Some(e)) => {
                let _ = sink.send(Progress {
                    job_id: 0,
                    stage: Stage::Removing,
                    percent: None,
                    bytes: None,
                    log: Some(format!("helper unavailable ({e}); falling back to pacman CLI")),
                    warning: true,
                });
                self.fallback_pacman(
                    &["-Rns", "--noconfirm", &id.name],
                    Stage::Removing,
                    sink,
                    cancel,
                )
            }
            Err(None) => Err(Error::Cancelled),
        }
    }

    fn upgrades(&self, sink: &ProgressSink, _cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        let local = Self::read_local_db();
        let sync_dbs = Self::read_all_sync_dbs();
        let mut results = Vec::new();

        for (name, local_pkg) in &local {
            for sync_pkgs in sync_dbs.values() {
                if let Some(sync_pkg) = sync_pkgs.get(name) {
                    if sync_pkg.version > local_pkg.version {
                        results.push(PackageSummary {
                            id: PackageId {
                                name: name.clone(),
                                source: Source::Repo,
                            },
                            version: sync_pkg.version.to_string(),
                            description: sync_pkg.description.to_string(),
                            groups: sync_pkg.groups.iter().map(|g| g.to_string()).collect(),
                            installed: true,
                            popular: None,
                            last_updated: None,
                        });
                    }
                    break;
                }
            }
        }

        results.sort_by(|a, b| a.id.name.cmp(&b.id.name));

        if results.is_empty() {
            sink.send(Progress {
                job_id: 0,
                stage: Stage::Verifying,
                percent: None,
                bytes: None,
                log: Some("repo: no upgrades available".into()),
                warning: false,
            })
            .ok();
        }

        Ok(results)
    }

    fn upgrade(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        self.install(id, sink, cancel)
    }

    fn upgrade_all(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let r = self.try_run_helper(&["sysupgrade"], Stage::Installing, sink, cancel);
        match r {
            Ok(()) => Ok(()),
            Err(Some(e)) => {
                let _ = sink.send(Progress {
                    job_id: 0,
                    stage: Stage::Installing,
                    percent: None,
                    bytes: None,
                    log: Some(format!("helper unavailable ({e}); falling back to pacman CLI")),
                    warning: true,
                });
                self.fallback_pacman(
                    &["-Syu", "--noconfirm"],
                    Stage::Installing,
                    sink,
                    cancel,
                )
            }
            Err(None) => Err(Error::Cancelled),
        }
    }

    fn cached_versions(&self, name: &str) -> Vec<(String, String)> {
        Self::cached_versions(name)
    }

    fn install_local(&self, path: &str, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        self.install_local(path, sink, cancel)
    }

    fn install_downgrade(&self, name: &str, version: &str, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        self.install_downgrade(name, version, sink, cancel)
    }

    fn cache_clean(&self, keep: u32, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        self.cache_clean(keep, sink, cancel)
    }

    fn available_groups(&self) -> Vec<String> {
        Self::available_groups()
    }

    fn orphans(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        let _ = sink;
        let _ = cancel;
        Self::orphans_list()
    }

    fn export(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<String> {
        let _ = sink;
        let _ = cancel;
        Self::export_list()
    }

    fn pacnew(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<Vec<PacnewFile>> {
        let _ = sink;
        let _ = cancel;
        Self::pacnew_list()
    }

    fn verify(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<String> {
        let _ = sink;
        let _ = cancel;
        Self::verify_packages()
    }

    fn list_installed(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        let _ = sink;
        let _ = cancel;
        Self::list_installed()
    }
}
