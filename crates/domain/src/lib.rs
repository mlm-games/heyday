use crossbeam_channel as chan;
use parking_lot::Mutex;
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime},
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
    pub groups: Vec<String>,
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
    PkgbuildContent {
        id: PackageId,
        content: String,
        token: PkgbuildToken,
    },
    CachedVersions {
        id: PackageId,
        versions: Vec<String>,
    },
    HistoryEntries(Vec<HistoryEntry>),
    Orphans(Vec<PackageSummary>),
    /// Exported package list as text.
    ExportResult(String),
    /// Detected .pacnew files.
    PacnewFiles(Vec<PacnewFile>),
    /// All available package groups across all repos.
    AvailableGroups(Vec<String>),
    /// All installed packages.
    InstalledPackages(Vec<PackageSummary>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct HistoryEntry {
    pub timestamp: SystemTime,
    pub kind: String,
    pub pkg: Option<String>,
    pub success: bool,
    pub message: String,
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
#[derive(Clone, Debug, PartialEq)]
pub struct PacnewFile {
    pub path: String,
    pub package: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RemoveOptions {
    pub cascade: bool,
    pub keep_config: bool,
    pub remove_optdeps: bool,
}

impl Default for RemoveOptions {
    fn default() -> Self {
        Self {
            cascade: true,
            keep_config: false,
            remove_optdeps: true,
        }
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

#[derive(Clone, Debug)]
pub struct PkgbuildToken(Arc<Mutex<Option<bool>>>);
impl PartialEq for PkgbuildToken {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl PkgbuildToken {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }
    pub fn approve(&self) {
        *self.0.lock() = Some(true);
    }
    pub fn reject(&self) {
        *self.0.lock() = Some(false);
    }
    pub fn wait(&self, cancel: &CancelToken) -> Result<bool> {
        loop {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            if let Some(decision) = *self.0.lock() {
                return Ok(decision);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

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
    /// Default: return empty (auto-approve, no review needed for repo).
    fn review_pkgbuild(
        &self,
        id: &PackageId,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> Result<String> {
        let _ = id;
        let _ = sink;
        let _ = cancel;
        Ok(String::new())
    }
    /// List cached versions available for downgrade.
    fn cached_versions(&self, _name: &str) -> Vec<(String, String)> {
        vec![]
    }
    /// Install a local package file.
    fn install_local(
        &self,
        _path: &str,
        _sink: &ProgressSink,
        _cancel: &CancelToken,
    ) -> Result<()> {
        Err(Error::Internal("install_local not implemented".into()))
    }
    /// Install a specific cached version (downgrade).
    fn install_downgrade(
        &self,
        _name: &str,
        _version: &str,
        _sink: &ProgressSink,
        _cancel: &CancelToken,
    ) -> Result<()> {
        Err(Error::Internal("install_downgrade not implemented".into()))
    }
    /// Clean pacman cache, keeping N versions per package.
    fn cache_clean(&self, _keep: u32, _sink: &ProgressSink, _cancel: &CancelToken) -> Result<()> {
        Err(Error::Internal("cache_clean not implemented".into()))
    }
    /// List all available package groups across repos.
    fn available_groups(&self) -> Vec<String> {
        vec![]
    }
    /// List all installed packages.
    fn list_installed(
        &self,
        _sink: &ProgressSink,
        _cancel: &CancelToken,
    ) -> Result<Vec<PackageSummary>> {
        Err(Error::Internal("list_installed not implemented".into()))
    }
    /// List orphaned packages (unused deps).
    fn orphans(&self, _sink: &ProgressSink, _cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        Err(Error::Internal("orphans not implemented".into()))
    }
    /// Export installed packages as text.
    fn export(&self, _sink: &ProgressSink, _cancel: &CancelToken) -> Result<String> {
        Err(Error::Internal("export not implemented".into()))
    }
    /// List .pacnew files.
    fn pacnew(&self, _sink: &ProgressSink, _cancel: &CancelToken) -> Result<Vec<PacnewFile>> {
        Err(Error::Internal("pacnew not implemented".into()))
    }
    /// Verify installed packages, returning output text.
    fn verify(&self, _sink: &ProgressSink, _cancel: &CancelToken) -> Result<String> {
        Err(Error::Internal("verify not implemented".into()))
    }
}

#[derive(Clone, Copy, Debug)]
pub enum JobKind {
    Refresh,
    Search,
    Details,
    Install,
    InstallLocal,
    Remove,
    Upgrades,
    Upgrade,
    UpgradeAll,
    CacheClean,
    Downgrade,
    History,
    Orphans,
    Export,
    Pacnew,
    Verify,
    ListInstalled,
}

#[derive(Clone, Debug)]
pub enum JobPayload {
    None,
    Query(String),
    Package(PackageId),
    PackageWithReview { id: PackageId, token: PkgbuildToken },
    InstallLocalFile(String),
    CacheCleanCount(u32),
    DowngradeVersion { id: PackageId, version: String },
    RemoveWithOptions { id: PackageId, options: RemoveOptions },
    Export,
    ExportFormat(String),
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

const HISTORY_FILE: &str = "soredowe/history.log";

pub fn append_history(kind: &str, pkg: Option<&str>, success: bool, msg: &str) {
    let dir = dirs_data_dir();
    if let Some(dir) = dir {
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(HISTORY_FILE);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            use std::io::Write;
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let pkg_field = pkg.unwrap_or("");
            let status = if success { "ok" } else { "fail" };
            let _ = writeln!(f, "{ts}|{kind}|{pkg_field}|{status}|{msg}");
        }
    }
}

pub fn read_history() -> std::io::Result<Vec<HistoryEntry>> {
    let mut entries = Vec::new();
    let dir = dirs_data_dir();
    let path = match dir {
        Some(d) => d.join(HISTORY_FILE),
        None => return Ok(entries),
    };
    let content = std::fs::read_to_string(path)?;
    for line in content.lines().rev().take(100) {
        let parts: Vec<&str> = line.splitn(5, '|').collect();
        if parts.len() < 5 {
            continue;
        }
        let secs: u64 = parts[0].parse().unwrap_or(0);
        let pkg = if parts[2].is_empty() {
            None
        } else {
            Some(parts[2].to_string())
        };
        entries.push(HistoryEntry {
            timestamp: std::time::UNIX_EPOCH + Duration::from_secs(secs),
            kind: parts[1].to_string(),
            pkg,
            success: parts[3] == "ok",
            message: parts[4].to_string(),
        });
    }
    Ok(entries)
}

fn dirs_data_dir() -> Option<std::path::PathBuf> {
    if let Ok(dir) = std::env::var("XDG_DATA_HOME") {
        Some(std::path::PathBuf::from(dir))
    } else if let Ok(dir) = std::env::var("HOME") {
        Some(std::path::PathBuf::from(dir).join(".local").join("share"))
    } else {
        None
    }
}

/// Score a package's relevance to a search query.
/// Returns a score where higher = more relevant.
/// Exact name match = 100, prefix match = 80, fuzzy name match = 60-40,
/// description substring = 20, fuzzy description = 10.
pub fn score_search(pkg: &PackageSummary, query: &str) -> u32 {
    let q = query.to_lowercase();
    let name = pkg.id.name.to_lowercase();
    let desc = pkg.description.to_lowercase();

    if name == q {
        return 100;
    }
    if name.starts_with(&q) {
        return 80;
    }
    let name_dist = strsim::levenshtein(&name, &q);
    let max_len = name.len().max(q.len()) as f64;
    if max_len > 0.0 {
        let name_sim = 1.0 - (name_dist as f64 / max_len);
        if name_sim >= 0.4 {
            return (40.0 + (name_sim * 20.0)) as u32;
        }
    }
    if desc.contains(&q) {
        return 20;
    }
    let desc_dist = strsim::levenshtein(
        &desc.chars().take(60).collect::<String>(),
        &q.chars().take(60).collect::<String>(),
    );
    let desc_max = 60.0_f64.max(q.len() as f64);
    if desc_max > 0.0 {
        let desc_sim = 1.0 - (desc_dist as f64 / desc_max);
        if desc_sim >= 0.5 {
            return 10;
        }
    }
    0
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

                            // Score and sort by relevance
                            let q_lower = q.to_lowercase();
                            items.sort_by(|a, b| {
                                let sa = score_search(a, &q_lower);
                                let sb = score_search(b, &q_lower);
                                sb.cmp(&sa).then_with(|| a.id.name.cmp(&b.id.name))
                            });
                            // Also send available groups
                            let groups = repo.available_groups();
                            let _ = tx_evt.send(Event::AvailableGroups(groups));

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
                            match &job.payload {
                                JobPayload::PackageWithReview { id, token }
                                    if id.source == Source::Aur =>
                                {
                                    // PKGBUILD review + install for AUR
                                    let content = aur.review_pkgbuild(id, &tx_local, &cancel)?;
                                    let _ = tx_evt.send(Event::PkgbuildContent {
                                        id: id.clone(),
                                        content,
                                        token: token.clone(),
                                    });
                                    let approved = token.wait(&cancel)?;
                                    if !approved {
                                        send_direct(
                                            Stage::Finished,
                                            Some("PKGBUILD review rejected".into()),
                                            false,
                                        );
                                        return Ok(());
                                    }
                                    aur.install(id, &tx_local, &cancel)
                                }
                                JobPayload::Package(id) => {
                                    pick(&job.payload).install(id, &tx_local, &cancel)
                                }
                                _ => Ok(()),
                            }
                        }
                        JobKind::InstallLocal => {
                            let _g = TXN_MUTEX.lock();
                            if let JobPayload::InstallLocalFile(path) = &job.payload {
                                repo.install_local(path, &tx_local, &cancel)
                            } else {
                                Ok(())
                            }
                        }
                        JobKind::CacheClean => {
                            let _g = TXN_MUTEX.lock();
                            let keep = match &job.payload {
                                JobPayload::CacheCleanCount(k) => *k,
                                _ => 3,
                            };
                            repo.cache_clean(keep, &tx_local, &cancel)
                        }
                        JobKind::Downgrade => {
                            let _g = TXN_MUTEX.lock();
                            match &job.payload {
                                JobPayload::DowngradeVersion { id, version } => {
                                    repo.install_downgrade(&id.name, version, &tx_local, &cancel)
                                }
                                _ => Ok(()),
                            }
                        }
                        JobKind::History => {
                            // Return history entries
                            let entries = read_history().unwrap_or_default();
                            let _ = tx_evt.send(Event::HistoryEntries(entries));
                            Ok(())
                        }
                        JobKind::Remove => {
                            let _g = TXN_MUTEX.lock();
                            match &job.payload {
                                JobPayload::Package(id) => {
                                    pick(&job.payload).remove(id, &tx_local, &cancel)
                                }
                                JobPayload::RemoveWithOptions { id, options } => {
                                    let _ = options;
                                    // TODO: pass remove options to backend
                                    repo.remove(id, &tx_local, &cancel)
                                }
                                _ => Ok(()),
                            }
                        }
                        JobKind::Orphans => {
                            // Collect orphans from repo backend
                            let orphans = repo.orphans(&tx_local, &cancel)?;
                            let _ = tx_evt.send(Event::Orphans(orphans));
                            Ok(())
                        }
                        JobKind::Export => {
                            let text = repo.export(&tx_local, &cancel)?;
                            let _ = tx_evt.send(Event::ExportResult(text));
                            Ok(())
                        }
                        JobKind::Pacnew => {
                            let files = repo.pacnew(&tx_local, &cancel)?;
                            let _ = tx_evt.send(Event::PacnewFiles(files));
                            Ok(())
                        }
                        JobKind::Verify => {
                            let results = repo.verify(&tx_local, &cancel)?;
                            // Reuse ExportResult for now — shows verification output
                            let _ = tx_evt.send(Event::ExportResult(results));
                            Ok(())
                        }
                        JobKind::ListInstalled => {
                            let items = repo.list_installed(&tx_local, &cancel)?;
                            let _ = tx_evt.send(Event::InstalledPackages(items));
                            // Also send available groups
                            let groups = repo.available_groups();
                            let _ = tx_evt.send(Event::AvailableGroups(groups));
                            Ok(())
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
                let is_ok = res.is_ok();
                if is_ok {
                    match job.kind {
                        JobKind::Install
                        | JobKind::InstallLocal
                        | JobKind::Remove
                        | JobKind::Upgrade
                        | JobKind::UpgradeAll
                        | JobKind::Downgrade
                        | JobKind::CacheClean => {
                            let _ = tx_evt.send(Event::SystemChanged);
                        }
                        _ => {}
                    }
                }
                // Log history for operations that change system state
                match job.kind {
                    JobKind::Install
                    | JobKind::InstallLocal
                    | JobKind::Remove
                    | JobKind::Upgrade
                    | JobKind::UpgradeAll
                    | JobKind::Downgrade
                    | JobKind::CacheClean => {
                        let pkg_name = match &job.payload {
                            JobPayload::Package(id) | JobPayload::PackageWithReview { id, .. } => {
                                Some(id.name.as_str())
                            }
                            JobPayload::DowngradeVersion { id, .. } => Some(id.name.as_str()),
                            _ => None,
                        };
                        let msg = match &res {
                            Ok(()) => "completed",
                            Err(e) => &e.to_string(),
                        };
                        append_history(
                            match job.kind {
                                JobKind::Install | JobKind::InstallLocal => "install",
                                JobKind::Remove => "remove",
                                JobKind::Upgrade => "upgrade",
                                JobKind::UpgradeAll => "sysupgrade",
                                JobKind::Downgrade => "downgrade",
                                JobKind::CacheClean => "cache-clean",
                                _ => "unknown",
                            },
                            pkg_name,
                            is_ok,
                            msg,
                        );
                    }
                    _ => {}
                }
                send_direct(
                    if is_ok {
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
