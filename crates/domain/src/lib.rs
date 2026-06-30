pub mod appstream;
pub mod config;

use crossbeam_channel as chan;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Source {
    Repo,
    Aur,
    Flatpak,
    AppImage,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageId {
    pub name: String,
    pub source: Source,
    pub repo: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PackageSummary {
    pub id: PackageId,
    pub version: String,
    pub description: String,
    pub installed: bool,
    pub popular: Option<u32>,
    pub last_updated: Option<SystemTime>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PackageDetails {
    pub summary: PackageSummary,
    pub description: Option<String>,
    pub depends: Vec<String>,
    pub opt_depends: Vec<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub maintainer: Option<String>,
    pub developer: Option<String>,
    pub size_install: Option<u64>,
    pub size_download: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stage {
    Queued,
    Refreshing,
    Searching,
    Resolving,
    Downloading,
    Building,
    Installing,
    Removing,
    Verifying,
    Cleaning,
    Finished,
    Failed,
}

#[derive(Clone, Debug)]
pub struct Progress {
    pub job_id: u64,
    pub stage: Stage,
    pub percent: Option<f32>,
    pub bytes: Option<(u64, u64)>,
    pub log: Option<String>,
    pub warning: bool,
}

#[derive(Clone, Debug)]
pub enum Event {
    SearchResults {
        query: String,
        items: Vec<PackageSummary>,
    },
    Details {
        item: PackageDetails,
    },
    Upgrades {
        items: Vec<PackageSummary>,
    },
    SystemChanged,
    Error(String),
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("network: {0}")]
    Network(String),
    #[error("alpm: {0}")]
    Alpm(String),
    #[error("aur: {0}")]
    Aur(String),
    #[error("flatpak: {0}")]
    Flatpak(String),
    #[error("packagekit: {0}")]
    PackageKit(String),
    #[error("appimage: {0}")]
    AppImage(String),
    #[error("privilege: {0}")]
    Priv(String),
    #[error("cancelled")]
    Cancelled,
    #[error("internal: {0}")]
    Internal(String),
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug)]
pub struct CancelToken(Arc<AtomicBool>);
impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}
impl CancelToken {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst)
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}
pub type ProgressSink = chan::Sender<Progress>;

/// A backend that manages a package source (repo, aur, flatpak, appimage).
pub trait PackageBackend: Send + Sync {
    fn name(&self) -> &'static str;

    fn refresh(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<()>;

    fn search(
        &self,
        q: &str,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> Result<Vec<PackageSummary>>;

    fn details(
        &self,
        id: &PackageId,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> Result<PackageDetails>;

    /// List all installed packages from this backend.
    fn installed(&self, cancel: &CancelToken) -> Result<Vec<PackageSummary>>;

    /// List all available updates from this backend.
    fn updates(&self, cancel: &CancelToken) -> Result<Vec<PackageSummary>>;

    /// Execute a privileged operation with progress reporting.
    fn operation(
        &self,
        op: &Operation,
        sink: &ProgressSink,
        cancel: &CancelToken,
        progress: Box<dyn FnMut(f32) + Send + 'static>,
    ) -> Result<()>;

    fn install(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()>;

    fn remove(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()>;

    fn upgrade(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()>;

    fn upgrade_all(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<()>;

    /// Install a local file (AppImage, flatpakref, pkg.tar.zst, etc.)
    fn install_file(&self, _path: &str, _sink: &ProgressSink, _cancel: &CancelToken) -> Result<()> {
        Err(Error::Internal("install_file not supported".into()))
    }
}

/// What kind of operation to perform.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum OperationKind {
    Install,
    Remove { purge_data: bool },
    Update,
    Refresh,
}

/// A package management operation.
#[derive(Clone, Debug)]
pub struct Operation {
    pub kind: OperationKind,
    pub backend_name: &'static str,
    pub package_ids: Vec<PackageId>,
}

#[derive(Clone, Copy, Debug)]
pub enum JobKind {
    Refresh,
    Search,
    Details,
    Install,
    Remove,
    Upgrades,
    Upgrade,
    UpgradeAll,
    InstallFile,
}

#[derive(Clone, Debug)]
pub enum JobPayload {
    None,
    Query(String),
    Package(PackageId),
    InstallFile(String),
}

#[derive(Clone, Debug)]
pub struct Job {
    pub id: u64,
    pub kind: JobKind,
    pub payload: JobPayload,
    pub created_at: SystemTime,
    pub cancel: CancelToken,
}

fn backend_matches(name: &str, source: &Source) -> bool {
    match source {
        Source::Repo => name == "alpm" || name == "packagekit",
        Source::Aur => name == "aur",
        Source::Flatpak => name == "flatpak",
        Source::AppImage => name == "appimage",
    }
}

static TXN_MUTEX: Mutex<()> = Mutex::new(());

/// Returns the set of currently installed package names (via `pacman -Qq`).
pub fn installed_package_names() -> HashSet<String> {
    let out = std::process::Command::new("pacman")
        .args(["-Qq"])
        .output()
        .ok();
    let mut set = HashSet::new();
    if let Some(out) = out {
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let n = line.trim();
            if !n.is_empty() {
                set.insert(n.to_string());
            }
        }
    }
    set
}

pub struct Executor {
    backends: Vec<(&'static str, Arc<dyn PackageBackend>)>,
    tx_prog: chan::Sender<Progress>,
    tx_evt: chan::Sender<Event>,
    rx_jobs: chan::Receiver<Job>,
    active_search_id: Option<u64>,
}

impl Executor {
    pub fn new(
        backends: Vec<(&'static str, Arc<dyn PackageBackend>)>,
        tx_prog: chan::Sender<Progress>,
        tx_evt: chan::Sender<Event>,
        rx_jobs: chan::Receiver<Job>,
    ) -> Self {
        Self {
            backends,
            tx_prog,
            tx_evt,
            rx_jobs,
            active_search_id: None,
        }
    }

    pub fn run(mut self) {
        std::thread::spawn(move || {
            while let Ok(job) = self.rx_jobs.recv() {
                let tx_prog = self.tx_prog.clone();
                let tx_evt = self.tx_evt.clone();
                let cancel = job.cancel.clone();

                let (tx_local, rx_local) = chan::unbounded::<Progress>();
                let fwd = {
                    let tx_prog = tx_prog.clone();
                    let jid = job.id;
                    std::thread::spawn(move || {
                        for mut p in rx_local.iter() {
                            p.job_id = jid;
                            let _ = tx_prog.send(p);
                        }
                    })
                };

                // Helper for executor-originated progress messages
                let send_direct = |stage: Stage, log: Option<String>, warning: bool| {
                    let _ = tx_prog.send(Progress {
                        job_id: job.id,
                        stage,
                        percent: None,
                        bytes: None,
                        log,
                        warning,
                    });
                };

                let backends: Vec<&dyn PackageBackend> =
                    self.backends.iter().map(|(_, b)| &**b).collect();
                let selected = &self.backends;
                let active_id = &mut self.active_search_id;

                send_direct(Stage::Queued, None, false);

                let mut run_job = |backends: &[&dyn PackageBackend]| -> Result<()> {
                    match job.kind {
                        JobKind::Refresh => {
                            for b in backends {
                                b.refresh(&tx_local, &cancel)?;
                            }
                            Ok(())
                        }
                        JobKind::Search => {
                            *active_id = Some(job.id);
                            let q = if let JobPayload::Query(q) = &job.payload {
                                q.trim().to_string()
                            } else {
                                String::new()
                            };
                            if q.len() < 2 {
                                let _ = tx_evt.send(Event::SearchResults {
                                    query: q,
                                    items: vec![],
                                });
                                return Ok(());
                            }

                            let mut items: Vec<PackageSummary> = Vec::new();
                            for b in backends {
                                match b.search(&q, &tx_local, &cancel) {
                                    Ok(mut v) => items.append(&mut v),
                                    Err(e) => {
                                        let _ = tx_evt.send(Event::Error(format!(
                                            "{} search failed: {e}",
                                            b.name()
                                        )));
                                    }
                                }
                            }

                            items.sort_by(|a, b| a.id.name.cmp(&b.id.name));
                            if *active_id == Some(job.id) {
                                tx_evt
                                    .send(Event::SearchResults { query: q, items })
                                    .map_err(|e| Error::Internal(e.to_string()))?;
                            }
                            Ok(())
                        }
                        JobKind::Details => {
                            if let JobPayload::Package(id) = &job.payload {
                                if let Some(b) = selected
                                    .iter()
                                    .find(|(name, _)| backend_matches(name, &id.source))
                                    .map(|(_, b)| &**b)
                                {
                                    let det = b.details(id, &tx_local, &cancel)?;
                                    tx_evt
                                        .send(Event::Details { item: det })
                                        .map_err(|e| Error::Internal(e.to_string()))?;
                                }
                            }
                            Ok(())
                        }
                        JobKind::Install => {
                            let _g = TXN_MUTEX.lock();
                            if let JobPayload::Package(id) = &job.payload {
                                if let Some(b) = selected
                                    .iter()
                                    .find(|(name, _)| backend_matches(name, &id.source))
                                    .map(|(_, b)| &**b)
                                {
                                    b.install(id, &tx_local, &cancel)
                                } else {
                                    Ok(())
                                }
                            } else {
                                Ok(())
                            }
                        }
                        JobKind::Remove => {
                            let _g = TXN_MUTEX.lock();
                            if let JobPayload::Package(id) = &job.payload {
                                if let Some(b) = selected
                                    .iter()
                                    .find(|(name, _)| backend_matches(name, &id.source))
                                    .map(|(_, b)| &**b)
                                {
                                    b.remove(id, &tx_local, &cancel)
                                } else {
                                    Ok(())
                                }
                            } else {
                                Ok(())
                            }
                        }
                        JobKind::Upgrades => {
                            let mut items: Vec<PackageSummary> = Vec::new();
                            for b in backends {
                                match b.updates(&cancel) {
                                    Ok(mut v) => items.append(&mut v),
                                    Err(e) => {
                                        send_direct(
                                            Stage::Verifying,
                                            Some(format!("{} upgrades failed: {e}", b.name())),
                                            true,
                                        );
                                    }
                                }
                            }
                            items.sort_by(|a, b| a.id.name.cmp(&b.id.name));
                            tx_evt
                                .send(Event::Upgrades { items })
                                .map_err(|e| Error::Internal(e.to_string()))?;
                            Ok(())
                        }
                        JobKind::Upgrade => {
                            let _g = TXN_MUTEX.lock();
                            if let JobPayload::Package(id) = &job.payload {
                                if let Some(b) = selected
                                    .iter()
                                    .find(|(name, _)| backend_matches(name, &id.source))
                                    .map(|(_, b)| &**b)
                                {
                                    b.upgrade(id, &tx_local, &cancel)
                                } else {
                                    Ok(())
                                }
                            } else {
                                Ok(())
                            }
                        }
                        JobKind::UpgradeAll => {
                            let _g = TXN_MUTEX.lock();
                            for b in backends {
                                b.upgrade_all(&tx_local, &cancel)?;
                            }
                            Ok(())
                        }
                        JobKind::InstallFile => {
                            if let JobPayload::InstallFile(path) = &job.payload {
                                let ext = std::path::Path::new(path)
                                    .extension()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("");
                                let backend = match ext {
                                    "AppImage" => selected
                                        .iter()
                                        .find(|(name, _)| *name == "appimage")
                                        .map(|(_, b)| &**b),
                                    "zst" if path.ends_with(".pkg.tar.zst") => selected
                                        .iter()
                                        .find(|(name, _)| *name == "alpm")
                                        .map(|(_, b)| &**b),
                                    _ => None,
                                };
                                if let Some(b) = backend {
                                    b.install_file(path, &tx_local, &cancel)
                                } else {
                                    send_direct(
                                        Stage::Failed,
                                        Some(format!("no backend for file: {path}")),
                                        true,
                                    );
                                    Err(Error::Internal(format!("no backend for file: {path}")))
                                }
                            } else {
                                Ok(())
                            }
                        }
                    }
                };

                let res = run_job(&backends);
                drop(tx_local);
                fwd.join().ok();
                if res.is_ok() {
                    match job.kind {
                        JobKind::Install
                        | JobKind::Remove
                        | JobKind::Upgrade
                        | JobKind::UpgradeAll
                        | JobKind::InstallFile => {
                            let _ = tx_evt.send(Event::SystemChanged);
                        }
                        _ => {}
                    }
                }
                send_direct(
                    if res.is_ok() {
                        Stage::Finished
                    } else {
                        Stage::Failed
                    },
                    res.as_ref().err().map(|e| e.to_string()),
                    res.is_err(),
                );
            }
        });
    }
}
