use domain::*;
use parking_lot::Mutex;
use regex::Regex;
use std::{
    cmp::Ordering,
    collections::HashSet,
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::{Arc, LazyLock},
};

/// SAFETY: `alpm::Alpm` is `!Send` + `!Sync`, but it is wrapped it in a `Mutex` and only accessed
/// when locked, on the executor thread.
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
            let _ = h.register_syncdb(
                name.as_str(),
                alpm::SigLevel::PACKAGE_OPTIONAL | alpm::SigLevel::DATABASE_OPTIONAL,
            );
        }
        *guard = Some(h);
        Ok(())
    }
}

fn pacman_conf_repos() -> Vec<String> {
    let content = match std::fs::read_to_string("/etc/pacman.conf") {
        Ok(c) => c,
        Err(_) => return vec!["core".into(), "extra".into()],
    };
    let mut repos = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if let Some(inner) = t
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
            .filter(|s| !s.eq_ignore_ascii_case("options"))
        {
            repos.push(inner.to_string());
        }
    }
    if repos.is_empty() {
        vec!["core".into(), "extra".into()]
    } else {
        repos
    }
}

fn installed_names(_handle: &alpm::Alpm) -> HashSet<String> {
    domain::installed_package_names()
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
            if let Some((cur, total, stage)) = parse_pacman_progress(&l) {
                let _ = tx2.send(Progress {
                    job_id: 0,
                    stage,
                    percent: Some(cur as f32 / total as f32),
                    bytes: None,
                    log: Some(l),
                    warning: false,
                });
            } else {
                let _ = tx2.send(Progress {
                    job_id: 0,
                    stage: default_stage,
                    percent: None,
                    bytes: None,
                    log: Some(l),
                    warning: true,
                });
            }
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

fn pkexec_pacman(
    args: &[&str],
    sink: &ProgressSink,
    cancel: &CancelToken,
    stage: Stage,
) -> Result<()> {
    let mut cmd = Command::new("pkexec");
    cmd.env("LC_ALL", "C");
    cmd.arg("pacman");
    cmd.args(args);
    let (code, last_err) = run_stream(cmd, sink, cancel, stage)?;
    if code == 0 {
        Ok(())
    } else {
        let why = last_err.map(|e| format!(": {e}")).unwrap_or_default();
        Err(Error::Priv(format!("pacman exit {code}{why}")))
    }
}

fn pkexec_pacman_args(
    args: &[&str],
    pkgs: Vec<String>,
    sink: &ProgressSink,
    cancel: &CancelToken,
    stage: Stage,
) -> Result<()> {
    let mut cmd = Command::new("pkexec");
    cmd.env("LC_ALL", "C");
    cmd.arg("pacman");
    cmd.args(args);
    cmd.args(&pkgs);
    let (code, last_err) = run_stream(cmd, sink, cancel, stage)?;
    if code == 0 {
        Ok(())
    } else {
        let why = last_err.map(|e| format!(": {e}")).unwrap_or_default();
        Err(Error::Priv(format!("pacman exit {code}{why}")))
    }
}

impl PackageBackend for AlpmBackend {
    fn name(&self) -> &'static str {
        "alpm"
    }

    fn group(&self) -> &'static str {
        "repo"
    }

    fn install(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let pkg = id
            .repo
            .as_ref()
            .map(|r| format!("{r}/{}", id.name))
            .unwrap_or_else(|| id.name.clone());
        pkexec_pacman(
            &["-S", "--noconfirm", "--needed", &pkg],
            sink,
            cancel,
            Stage::Installing,
        )
    }

    fn refresh(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        match pkexec_pacman(&["-Sy", "--noconfirm"], sink, cancel, Stage::Refreshing) {
            Ok(()) => Ok(()),
            Err(Error::Internal(e)) if e.contains("spawn:") => {
                send_log(
                    sink,
                    Stage::Refreshing,
                    "pkexec unavailable; trying unprivileged refresh",
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
            send_log(
                sink,
                Stage::Searching,
                "repo: query too short (<2), ignoring",
                true,
            );
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

        send_log(
            sink,
            Stage::Searching,
            &format!("repo: {} matches", results.len()),
            false,
        );
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

                let long_desc = pkg.desc().unwrap_or("").to_string();
                return Ok(PackageDetails {
                    summary,
                    description: Some(long_desc).filter(|d| !d.is_empty()),
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
                    license: pkg.licenses().first().map(|s| s.to_string()),
                    maintainer: pkg.packager().map(|s| s.to_string()),
                    developer: None,
                    size_install: Some(pkg.isize() as u64),
                    size_download: Some(pkg.download_size() as u64),
                });
            }
        }

        send_log(
            sink,
            Stage::Searching,
            &format!("repo: package not found: {}", id.name),
            true,
        );
        Err(Error::Alpm("not found".into()))
    }

    fn installed(&self, _cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        // Re-init handle for fresh localdb after external pkexec operations
        *self.handle.lock() = None;
        self.ensure()?;
        let guard = self.handle.lock();
        let handle = guard.as_ref().expect("alpm handle");
        Ok(handle
            .localdb()
            .pkgs()
            .into_iter()
            .map(|p| PackageSummary {
                id: PackageId {
                    name: p.name().to_string(),
                    source: Source::Repo,
                    repo: None,
                },
                version: p.version().as_str().to_string(),
                description: p.desc().unwrap_or("").to_string(),
                installed: true,
                popular: None,
                last_updated: None,
            })
            .collect())
    }

    fn updates(&self, _cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        // Re-init handle for fresh localdb after external pkexec operations
        *self.handle.lock() = None;
        self.ensure()?;

        let guard = self.handle.lock();
        let handle = guard.as_ref().expect("alpm handle");
        let local = handle.localdb();
        let mut results = Vec::new();

        for pkg in local.pkgs() {
            let name = pkg.name();
            let mut best: Option<(String, String, String)> = None;
            for db in handle.syncdbs() {
                if let Ok(sync_pkg) = db.pkg(name) {
                    let sv = sync_pkg.version().as_str().to_string();
                    let better = best.as_ref().map_or(true, |(_, bv, _)| {
                        alpm::vercmp(bv.as_str(), sv.as_str()) == Ordering::Less
                    });
                    if better {
                        best = Some((
                            db.name().to_string(),
                            sv,
                            sync_pkg.desc().unwrap_or("").to_string(),
                        ));
                    }
                }
            }
            if let Some((repo, version, desc)) = best {
                let local_v = pkg.version().as_str();
                if alpm::vercmp(local_v, &version) == Ordering::Less {
                    results.push(PackageSummary {
                        id: PackageId {
                            name: name.to_string(),
                            source: Source::Repo,
                            repo: Some(repo),
                        },
                        version,
                        description: desc,
                        installed: true,
                        popular: None,
                        last_updated: None,
                    });
                }
            }
        }
        Ok(results)
    }

    fn remove(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        pkexec_pacman(
            &["-Rns", "--noconfirm", &id.name],
            sink,
            cancel,
            Stage::Removing,
        )
    }

    fn upgrade(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let pkg = id
            .repo
            .as_ref()
            .map(|r| format!("{r}/{}", id.name))
            .unwrap_or_else(|| id.name.clone());
        pkexec_pacman(
            &["-S", "--noconfirm", &pkg],
            sink,
            cancel,
            Stage::Installing,
        )
    }

    fn upgrade_all(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        pkexec_pacman(&["-Syu", "--noconfirm"], sink, cancel, Stage::Installing)
    }

    fn install_file(&self, path: &str, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        pkexec_pacman(
            &["-U", "--noconfirm", path],
            sink,
            cancel,
            Stage::Installing,
        )
    }

    fn operation(
        &self,
        op: &Operation,
        sink: &ProgressSink,
        cancel: &CancelToken,
        _progress: Box<dyn FnMut(f32) + Send + 'static>,
    ) -> Result<()> {
        match op.kind {
            OperationKind::Install => {
                let pkgs: Vec<String> = op
                    .package_ids
                    .iter()
                    .map(|id| {
                        id.repo
                            .as_ref()
                            .map(|r| format!("{r}/{}", id.name))
                            .unwrap_or_else(|| id.name.clone())
                    })
                    .collect();
                if pkgs.is_empty() {
                    return Err(Error::Internal("no packages".into()));
                }
                pkexec_pacman_args(
                    &["-S", "--noconfirm", "--needed"],
                    pkgs,
                    sink,
                    cancel,
                    Stage::Installing,
                )
            }
            OperationKind::Remove { .. } => {
                let pkgs: Vec<String> = op.package_ids.iter().map(|id| id.name.clone()).collect();
                if pkgs.is_empty() {
                    return Err(Error::Internal("no packages".into()));
                }
                pkexec_pacman_args(
                    &["-Rns", "--noconfirm"],
                    pkgs,
                    sink,
                    cancel,
                    Stage::Removing,
                )
            }
            OperationKind::Update => {
                let pkgs: Vec<String> = op
                    .package_ids
                    .iter()
                    .map(|id| {
                        id.repo
                            .as_ref()
                            .map(|r| format!("{r}/{}", id.name))
                            .unwrap_or_else(|| id.name.clone())
                    })
                    .collect();
                if pkgs.is_empty() {
                    return Err(Error::Internal("no packages".into()));
                }
                pkexec_pacman_args(
                    &["-S", "--noconfirm"],
                    pkgs,
                    sink,
                    cancel,
                    Stage::Installing,
                )
            }
            OperationKind::Refresh => self.refresh(sink, cancel),
        }
    }
}
