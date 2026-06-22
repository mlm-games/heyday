use crossbeam_channel as chan;
use parking_lot::Mutex;
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    Repo,
    Aur,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PackageId {
    pub name: String,
    pub source: Source,
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
    pub depends: Vec<String>,
    pub opt_depends: Vec<String>,
    pub homepage: Option<String>,
    pub maintainer: Option<String>,
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
    /// Sent when the system package state likely changed (install/remove/upgrade).
    SystemChanged,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("network: {0}")]
    Network(String),
    #[error("alpm: {0}")]
    Alpm(String),
    #[error("aur: {0}")]
    Aur(String),
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

pub trait PackageBackend: Send + Sync {
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
    fn install(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()>;
    fn remove(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()>;
    fn upgrades(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<Vec<PackageSummary>>;
    fn upgrade(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()>;
    fn upgrade_all(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<()>;
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
}

#[derive(Clone, Debug)]
pub enum JobPayload {
    None,
    Query(String),
    Package(PackageId),
}

#[derive(Clone, Debug)]
pub struct Job {
    pub id: u64,
    pub kind: JobKind,
    pub payload: JobPayload,
    pub created_at: SystemTime,
    pub cancel: CancelToken,
}

static TXN_MUTEX: Mutex<()> = Mutex::new(());

/// Returns the set of currently installed package names (via `pacman -Qq`).
pub fn installed_package_names() -> HashSet<String> {
    let out = std::process::Command::new("pacman").args(["-Qq"]).output().ok();
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
    repo: Arc<dyn PackageBackend>,
    aur: Arc<dyn PackageBackend>,
    tx_prog: chan::Sender<Progress>,
    tx_evt: chan::Sender<Event>,
    rx_jobs: chan::Receiver<Job>,
    active_search_id: Option<u64>,
}

impl Executor {
    pub fn new(
        repo: Arc<dyn PackageBackend>,
        aur: Arc<dyn PackageBackend>,
        tx_prog: chan::Sender<Progress>,
        tx_evt: chan::Sender<Event>,
        rx_jobs: chan::Receiver<Job>,
    ) -> Self {
        Self {
            repo,
            aur,
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

                // Per-job progress channel: backends write here; we inject job_id and forward.
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

                let repo = &self.repo;
                let aur = &self.aur;
                let active_id = &mut self.active_search_id;
                let pick = |payload: &JobPayload| -> &dyn PackageBackend {
                    match payload {
                        JobPayload::Package(id) if id.source == Source::Aur => &**aur,
                        _ => &**repo,
                    }
                };

                send_direct(Stage::Queued, None, false);

                let mut run_job = || -> Result<()> {
                    match job.kind {
                        JobKind::Refresh => pick(&job.payload).refresh(&tx_local, &cancel),
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

                            let mut any_ok = false;
                            let mut items: Vec<PackageSummary> = Vec::new();

                            // Repo
                            match repo.search(&q, &tx_local, &cancel) {
                                Ok(mut v) => {
                                    items.append(&mut v);
                                    any_ok = true;
                                }
                                Err(e) => {
                                    send_direct(
                                        Stage::Searching,
                                        Some(format!("repo search failed: {e}")),
                                        true,
                                    );
                                }
                            }

                            // AUR
                            match aur.search(&q, &tx_local, &cancel) {
                                Ok(mut v) => {
                                    items.append(&mut v);
                                    any_ok = true;
                                }
                                Err(e) => {
                                    send_direct(
                                        Stage::Searching,
                                        Some(format!("AUR search failed: {e}")),
                                        true,
                                    );
                                }
                            }

                            // If both failed, bubble a failure to the final Progress; otherwise continue.
                            if !any_ok {
                                return Err(Error::Alpm("all backends failed".into()));
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
                                let det = pick(&job.payload).details(id, &tx_local, &cancel)?;
                                tx_evt
                                    .send(Event::Details { item: det })
                                    .map_err(|e| Error::Internal(e.to_string()))?;
                            }
                            Ok(())
                        }
                        JobKind::Install => {
                            let _g = TXN_MUTEX.lock();
                            if let JobPayload::Package(id) = &job.payload {
                                pick(&job.payload).install(id, &tx_local, &cancel)
                            } else {
                                Ok(())
                            }
                        }
                        JobKind::Remove => {
                            let _g = TXN_MUTEX.lock();
                            if let JobPayload::Package(id) = &job.payload {
                                pick(&job.payload).remove(id, &tx_local, &cancel)
                            } else {
                                Ok(())
                            }
                        }
                        JobKind::Upgrades => {
                            // Collect from both repo and AUR, but don’t fail the whole job
                            let mut items: Vec<PackageSummary> = Vec::new();
                            match repo.upgrades(&tx_local, &cancel) {
                                Ok(mut v) => items.append(&mut v),
                                Err(e) => {
                                    send_direct(
                                        Stage::Verifying,
                                        Some(format!("repo upgrades failed: {e}")),
                                        true,
                                    );
                                }
                            }
                            match aur.upgrades(&tx_local, &cancel) {
                                Ok(mut v) => items.append(&mut v),
                                Err(e) => {
                                    send_direct(
                                        Stage::Verifying,
                                        Some(format!("AUR upgrades failed: {e}")),
                                        true,
                                    );
                                }
                            }
                            // Sort A-Z for stability; UI can re-sort
                            items.sort_by(|a, b| a.id.name.cmp(&b.id.name));
                            tx_evt
                                .send(Event::Upgrades { items })
                                .map_err(|e| Error::Internal(e.to_string()))?;
                            Ok(())
                        }
                        JobKind::Upgrade => {
                            let _g = TXN_MUTEX.lock();
                            if let JobPayload::Package(id) = &job.payload {
                                pick(&job.payload).upgrade(id, &tx_local, &cancel)
                            } else {
                                Ok(())
                            }
                        }
                        JobKind::UpgradeAll => {
                            let _g = TXN_MUTEX.lock();
                            // Minimal: perform repo full system upgrade; AUR can be expanded later.
                            repo.upgrade_all(&tx_local, &cancel)?;
                            // If you want AUR mass-upgrade later, we can iterate aur.upgrades() and call aur.upgrade(..).
                            Ok(())
                        }
                    }
                };

                let res = run_job();
                drop(tx_local);
                fwd.join().ok(); // If forwarding thread panicked, progress stops but job result is unaffected.
                if res.is_ok() {
                    match job.kind {
                        JobKind::Install
                        | JobKind::Remove
                        | JobKind::Upgrade
                        | JobKind::UpgradeAll => {
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


