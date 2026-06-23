use alpm::SigLevel;
use domain::*;
use parking_lot::Mutex;
use regex::Regex;
use std::{
    collections::HashSet,
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::{Arc, LazyLock},
};

pub struct AlpmBackend {
    handle: Mutex<Option<alpm::Alpm>>,
}

unsafe impl Send for AlpmBackend {}
unsafe impl Sync for AlpmBackend {}

impl Default for AlpmBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl AlpmBackend {
    pub fn new() -> Self {
        Self {
            handle: Mutex::new(None),
        }
    }

    fn ensure(&self) -> Result<()> {
        let mut guard = self.handle.lock();
        if guard.is_some() {
            return Ok(());
        }
        let h = alpm::Alpm::new("/", "/var/lib/pacman")
            .map_err(|e| Error::Internal(format!("alpm init: {e}")))?;

        let names = pacman_conf_repos();
        for name in &names {
            let _ = h.register_syncdb(name.as_str(), SigLevel::NONE);
        }
        *guard = Some(h);
        Ok(())
    }
}

/// Parse `/etc/pacman.conf` for `[repo]` sections, excluding `[options]`.
fn pacman_conf_repos() -> Vec<String> {
    let content = match std::fs::read_to_string("/etc/pacman.conf") {
        Ok(c) => c,
        Err(_) => return vec!["core".into(), "extra".into(), "community".into()],
    };
    let mut repos = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if let Some(inner) = t.strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
            .filter(|s| !s.eq_ignore_ascii_case("options"))
        {
            repos.push(inner.to_string());
        }
    }
    if repos.is_empty() {
        vec!["core".into(), "extra".into(), "community".into()]
    } else {
        repos
    }
}

fn installed_names(handle: &alpm::Alpm) -> HashSet<String> {
    handle
        .localdb()
        .pkgs()
        .into_iter()
        .map(|p| p.name().to_string())
        .collect()
}

static PACMAN_PROGRESS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\((\d+)/(\d+)\)\s+(.+)").unwrap());

fn parse_pacman_progress(line: &str) -> Option<(u32, u32, Stage)> {
    let caps = PACMAN_PROGRESS.captures(line)?;
    let cur: u32 = caps[1].parse().ok()?;
    let total: u32 = caps[2].parse().ok()?;
    let text = caps[3].to_lowercase();
    let stage = if text.starts_with("download") {
        Stage::Downloading
    } else if text.starts_with("install") {
        Stage::Installing
    } else if text.starts_with("remove") {
        Stage::Removing
    } else if text.starts_with("check") || text.contains("hook") {
        Stage::Verifying
    } else if text.starts_with("load") {
        Stage::Installing
    } else {
        return None;
    };
    Some((cur, total, stage))
}

fn run_stream(
    mut cmd: Command,
    sink: &ProgressSink,
    cancel: &CancelToken,
    default_stage: Stage,
) -> Result<(i32, Option<String>)> {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Internal(format!("spawn: {e}")))?;
    let out = child.stdout.take().expect("stdout piped");
    let err = child.stderr.take().expect("stderr piped");

    let last_err = Arc::new(Mutex::new(None::<String>));
    let last_err_t2 = last_err.clone();
    let tx1 = sink.clone();
    let tx2 = sink.clone();

    let t1 = std::thread::spawn(move || {
        for l in BufReader::new(out).lines().map_while(|r| r.ok()) {
            let (stage, percent) = parse_pacman_progress(&l)
                .map(|(cur, total, stage)| (stage, Some(cur as f32 / total as f32)))
                .unwrap_or((default_stage, None));
            let _ = tx1.send(Progress {
                job_id: 0,
                stage,
                percent,
                bytes: None,
                log: Some(l),
                warning: false,
            });
        }
    });

    let t2 = std::thread::spawn(move || {
        for l in BufReader::new(err).lines().map_while(|r| r.ok()) {
            {
                let mut g = last_err_t2.lock();
                *g = Some(l.clone());
            }
            let (stage, percent) = parse_pacman_progress(&l)
                .map(|(cur, total, stage)| (stage, Some(cur as f32 / total as f32)))
                .unwrap_or((default_stage, None));
            let _ = tx2.send(Progress {
                job_id: 0,
                stage,
                percent,
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

fn send_log(sink: &ProgressSink, stage: Stage, msg: &str, warning: bool) {
    let _ = sink.send(Progress {
        job_id: 0,
        stage,
        percent: None,
        bytes: None,
        log: Some(msg.into()),
        warning,
    });
}

impl PackageBackend for AlpmBackend {
    fn install(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let pkg = id
            .repo
            .as_ref()
            .map(|r| format!("{r}/{}", id.name))
            .unwrap_or_else(|| id.name.clone());
        let mut cmd = Command::new("pkexec");
        cmd.args(["pacman", "-S", "--noconfirm", "--needed", &pkg]);
        let (code, last_err) = run_stream(cmd, sink, cancel, Stage::Installing)?;
        if code == 0 {
            Ok(())
        } else {
            let why = last_err.map(|e| format!(": {e}")).unwrap_or_default();
            Err(Error::Priv(format!("install exit {code}{why}")))
        }
    }

    fn refresh(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let try_pkexec = || {
            let mut cmd = Command::new("pkexec");
            cmd.args(["pacman", "-Sy", "--noconfirm"]);
            let (code, last_err) = run_stream(cmd, sink, cancel, Stage::Refreshing)?;
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
                send_log(
                    sink,
                    Stage::Refreshing,
                    "pkexec unavailable; attempting unprivileged refresh (may fail)",
                    true,
                );
                let mut cmd = Command::new("pacman");
                cmd.args(["-Sy", "--noconfirm"]);
                let (code, last_err) = run_stream(cmd, sink, cancel, Stage::Refreshing)?;
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
        cancel: &CancelToken,
    ) -> Result<Vec<PackageSummary>> {
        let q = q.trim().to_lowercase();
        if q.len() < 2 {
            send_log(sink, Stage::Searching, "repo: query too short (<2), ignoring", true);
            return Ok(vec![]);
        }
        send_log(sink, Stage::Searching, &format!("repo search: {q}"), false);
        self.ensure()?;

        let guard = self.handle.lock();
        let handle = guard.as_ref().expect("alpm handle");
        let installed = installed_names(handle);
        let mut results = Vec::new();

        for db in handle.syncdbs() {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            for pkg in db.pkgs() {
                if cancel.is_cancelled() {
                    return Err(Error::Cancelled);
                }
                let name = pkg.name();
                let desc = pkg.desc().unwrap_or("");
                if name.to_lowercase().contains(&q) || desc.to_lowercase().contains(&q) {
                    results.push(PackageSummary {
                        id: PackageId {
                            name: name.to_string(),
                            source: Source::Repo,
                            repo: Some(db.name().to_string()),
                        },
                        version: pkg.version().as_str().to_string(),
                        description: desc.to_string(),
                        installed: installed.contains(name),
                        popular: None,
                        last_updated: None,
                    });
                }
            }
        }

        send_log(sink, Stage::Searching, &format!("repo: {} matches", results.len()), false);
        Ok(results)
    }

    fn details(
        &self,
        id: &PackageId,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> Result<PackageDetails> {
        self.ensure()?;

        let guard = self.handle.lock();
        let handle = guard.as_ref().expect("alpm handle");
        let installed = installed_names(handle);

        for db in handle.syncdbs() {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            if let Ok(pkg) = db.pkg(id.name.as_str()) {
                let name = pkg.name();
                let summary = PackageSummary {
                    id: PackageId {
                        name: name.to_string(),
                        source: Source::Repo,
                        repo: Some(db.name().to_string()),
                    },
                    version: pkg.version().as_str().to_string(),
                    description: pkg.desc().unwrap_or("").to_string(),
                    installed: installed.contains(name),
                    popular: None,
                    last_updated: None,
                };

                return Ok(PackageDetails {
                    summary,
                    depends: pkg
                        .depends()
                        .into_iter()
                        .map(|d| d.name().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                    opt_depends: pkg
                        .optdepends()
                        .into_iter()
                        .map(|d| d.name().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                    homepage: pkg.url().map(|s| s.to_string()),
                    maintainer: pkg.packager().map(|s| s.to_string()),
                    size_install: Some(pkg.isize() as u64),
                    size_download: Some(pkg.download_size() as u64),
                });
            }
        }

        send_log(sink, Stage::Searching, &format!("repo: package not found: {}", id.name), true);
        Err(Error::Alpm("not found".into()))
    }

    fn remove(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let mut cmd = Command::new("pkexec");
        cmd.args(["pacman", "-Rns", "--noconfirm", &id.name]);
        let (code, last_err) = run_stream(cmd, sink, cancel, Stage::Removing)?;
        if code == 0 {
            Ok(())
        } else {
            let why = last_err.map(|e| format!(": {e}")).unwrap_or_default();
            Err(Error::Priv(format!("remove exit {code}{why}")))
        }
    }

    fn upgrades(
        &self,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> Result<Vec<PackageSummary>> {
        self.ensure()?;

        let guard = self.handle.lock();
        let handle = guard.as_ref().expect("alpm handle");
        let local = handle.localdb();
        let mut results = Vec::new();

        for pkg in local.pkgs() {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let name = pkg.name();
            for db in handle.syncdbs() {
                if let Ok(sync_pkg) = db.pkg(name) {
                    if alpm::vercmp(pkg.version().as_str(), sync_pkg.version().as_str())
                        == std::cmp::Ordering::Less
                    {
                        results.push(PackageSummary {
                            id: PackageId {
                                name: name.to_string(),
                                source: Source::Repo,
                                repo: Some(db.name().to_string()),
                            },
                            version: sync_pkg.version().as_str().to_string(),
                            description: sync_pkg.desc().unwrap_or("").to_string(),
                            installed: true,
                            popular: None,
                            last_updated: None,
                        });
                    }
                    break;
                }
            }
        }

        send_log(sink, Stage::Verifying, &format!("repo: {} upgrades", results.len()), false);
        Ok(results)
    }

    fn upgrade(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        self.install(id, sink, cancel)
    }

    fn upgrade_all(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let mut cmd = Command::new("pkexec");
        cmd.args(["pacman", "-Syu", "--noconfirm"]);
        let (code, last_err) = run_stream(cmd, sink, cancel, Stage::Installing)?;
        if code == 0 {
            Ok(())
        } else {
            let why = last_err.map(|e| format!(": {e}")).unwrap_or_default();
            Err(Error::Priv(format!("upgrade-all exit {code}{why}")))
        }
    }
}
