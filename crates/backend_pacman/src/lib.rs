use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader, Read},
    path::Path,
    process::{Command, Stdio},
    str::FromStr,
    sync::Arc,
};

use alpm_db::desc::DbDescFileV2;
use alpm_repo_db::desc::RepoDescFileV2;
use flate2::read::GzDecoder;
use parking_lot::Mutex;
use tar::Archive;

use domain::*;

const PACMAN_DB: &str = "/var/lib/pacman";

pub struct PacmanCli;

impl Default for PacmanCli {
    fn default() -> Self {
        Self::new()
    }
}

impl PacmanCli {
    pub fn new() -> Self {
        Self
    }

    fn read_local_db() -> HashMap<String, DbDescFileV2> {
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
            if let Ok(desc) = DbDescFileV2::from_str(&content) {
                packages.insert(desc.name.to_string(), desc);
            }
        }
        packages
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
        Self::read_local_db().into_keys().collect()
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

        let stage_out = stage.clone();
        let stage_err = stage;

        let last_err = Arc::new(Mutex::new(None::<String>));
        let last_err_t2 = last_err.clone();

        let t1 = std::thread::spawn(move || {
            #[allow(clippy::lines_filter_map_ok)]
            for l in BufReader::new(out).lines().filter_map(|r| r.ok()) {
                let _ = tx1.send(Progress {
                    job_id: jid,
                    stage: stage_out.clone(),
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
                    stage: stage_err.clone(),
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
}

impl PackageBackend for PacmanCli {
    fn install(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let mut cmd = Command::new("pkexec");
        cmd.args(["pacman", "-S", "--noconfirm", "--needed", &id.name]);
        let (code, last_err) = self.run_stream(cmd, sink, cancel, Stage::Installing)?;
        if code == 0 {
            Ok(())
        } else {
            let why = last_err.map(|e| format!(": {e}")).unwrap_or_default();
            Err(Error::Priv(format!("install exit {code}{why}")))
        }
    }

    fn refresh(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
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

        let q_lower = q.to_lowercase();
        let installed = Self::installed_names();
        let sync_dbs = Self::read_all_sync_dbs();

        let mut results: Vec<PackageSummary> = Vec::new();

        for packages in sync_dbs.values() {
            for pkg in packages.values() {
                let name_lower = pkg.name.to_string().to_lowercase();
                let desc_lower = pkg.description.to_string().to_lowercase();

                if name_lower.contains(&q_lower) || desc_lower.contains(&q_lower) {
                    results.push(PackageSummary {
                        id: PackageId {
                            name: pkg.name.to_string(),
                            source: Source::Repo,
                        },
                        version: pkg.version.to_string(),
                        description: pkg.description.to_string(),
                        installed: installed.contains(pkg.name.to_string().as_str()),
                        popular: None,
                        last_updated: None,
                    });
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
        let sync_dbs = Self::read_all_sync_dbs();

        for packages in sync_dbs.values() {
            if let Some(pkg) = packages.get(&id.name) {
                return Ok(PackageDetails {
                    summary: PackageSummary {
                        id: id.clone(),
                        version: pkg.version.to_string(),
                        description: pkg.description.to_string(),
                        installed: false,
                        popular: None,
                        last_updated: None,
                    },
                    depends: pkg.dependencies.iter().map(|d| d.to_string()).collect(),
                    opt_depends: pkg
                        .optional_dependencies
                        .iter()
                        .map(|d| d.to_string())
                        .collect(),
                    homepage: pkg.url.as_ref().map(|u| u.to_string()),
                    maintainer: Some(pkg.packager.to_string()),
                    size_install: Some(pkg.installed_size),
                    size_download: Some(pkg.compressed_size),
                });
            }
        }

        Err(Error::Alpm(format!(
            "package {} not found in repos",
            id.name
        )))
    }

    fn remove(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let mut cmd = Command::new("pkexec");
        cmd.args(["pacman", "-Rns", "--noconfirm", &id.name]);
        let (code, last_err) = self.run_stream(cmd, sink, cancel, Stage::Removing)?;
        if code == 0 {
            Ok(())
        } else {
            let why = last_err.map(|e| format!(": {e}")).unwrap_or_default();
            Err(Error::Priv(format!("remove exit {code}{why}")))
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
        let mut cmd = Command::new("pkexec");
        cmd.args(["pacman", "-Syu", "--noconfirm"]);
        let (code, last_err) = self.run_stream(cmd, sink, cancel, Stage::Installing)?;
        if code == 0 {
            Ok(())
        } else {
            let why = last_err.map(|e| format!(": {e}")).unwrap_or_default();
            Err(Error::Priv(format!("upgrade-all exit {code}{why}")))
        }
    }
}
