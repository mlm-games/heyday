use crossbeam_channel as chan;
use domain::*;
use repose_core::signal::signal;

const MAX_LOG: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortMode {
    NameAsc,
    NameDesc,
    Popularity,
}

impl Default for SortMode {
    fn default() -> Self {
        Self::Popularity
    }
}

#[derive(Clone, Debug, Default)]
pub struct AppState {
    pub query: String,
    pub results: Vec<PackageSummary>,
    pub selected: Option<PackageId>,
    pub filter_repo: bool,
    pub filter_aur: bool,
    pub filter_installed: bool,
    pub sort: SortMode,
    pub progress_log: String,
    pub error: Option<String>,
    pub log_expanded: bool,
    pub in_upgrades_view: bool,
}

#[derive(Clone, Debug)]
pub enum Action {
    SetQuery(String),
    Search,
    Refresh,
    Upgrades,
    UpgradeAll,
    Upgrade(PackageId),
    Install(PackageId),
    Remove(PackageId),
    Progress(Progress),
    Event(Event),
    ClearError,
    Select(PackageId),
    ClearSelection,
    ToggleFilterRepo,
    ToggleFilterAur,
    ToggleFilterInstalled,
    SetSort(SortMode),
    ToggleLog,
}

pub struct Store {
    pub state: repose_core::signal::Signal<AppState>,
    pub tx_jobs: chan::Sender<domain::Job>,
    next_id: std::sync::atomic::AtomicU64,
}
impl Store {
    pub fn new(tx_jobs: chan::Sender<domain::Job>) -> Self {
        let mut s = AppState::default();
        s.filter_repo = true;
        s.filter_aur = true;
        s.sort = SortMode::default();
        Self {
            state: signal(s),
            tx_jobs,
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }
    fn jid(&self) -> u64 {
        self.next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    pub fn dispatch(&self, a: Action) {
        let mut s = self.state.get();
        match a {
            Action::SetQuery(q) => s.query = q,
            Action::Search => {
                s.in_upgrades_view = false;
                let q = s.query.trim().to_string();

                let id = self.jid();
                let _ = self.tx_jobs.send(Job {
                    id,
                    kind: JobKind::Search,
                    payload: JobPayload::Query(q.clone()),
                    created_at: std::time::SystemTime::now(),
                    cancel: CancelToken::new(),
                });

                // Clear previous results if query is empty
                if q.is_empty() {
                    s.results.clear();
                    s.selected = None;
                }
            }
            Action::Refresh => {
                let id = self.jid();
                let _ = self.tx_jobs.send(Job {
                    id,
                    kind: JobKind::Refresh,
                    payload: JobPayload::None,
                    created_at: std::time::SystemTime::now(),
                    cancel: CancelToken::new(),
                });
                if s.in_upgrades_view {
                    let id = self.jid();
                    let _ = self.tx_jobs.send(Job {
                        id,
                        kind: JobKind::Upgrades,
                        payload: JobPayload::None,
                        created_at: std::time::SystemTime::now(),
                        cancel: CancelToken::new(),
                    });
                } else if !s.query.trim().is_empty() {
                    let id = self.jid();
                    let q = s.query.clone();
                    let _ = self.tx_jobs.send(Job {
                        id,
                        kind: JobKind::Search,
                        payload: JobPayload::Query(q),
                        created_at: std::time::SystemTime::now(),
                        cancel: CancelToken::new(),
                    });
                }
            }
            Action::Upgrades => {
                s.in_upgrades_view = true;
                let id = self.jid();
                let _ = self.tx_jobs.send(Job {
                    id,
                    kind: JobKind::Upgrades,
                    payload: JobPayload::None,
                    created_at: std::time::SystemTime::now(),
                    cancel: CancelToken::new(),
                });
            }
            Action::UpgradeAll => {
                let id = self.jid();
                let _ = self.tx_jobs.send(Job {
                    id,
                    kind: JobKind::UpgradeAll,
                    payload: JobPayload::None,
                    created_at: std::time::SystemTime::now(),
                    cancel: CancelToken::new(),
                });
            }
            Action::Upgrade(id) => {
                let jid = self.jid();
                let _ = self.tx_jobs.send(Job {
                    id: jid,
                    kind: JobKind::Upgrade,
                    payload: JobPayload::Package(id),
                    created_at: std::time::SystemTime::now(),
                    cancel: CancelToken::new(),
                });
            }

            Action::Install(id) => {
                let jid = self.jid();
                let _ = self.tx_jobs.send(Job {
                    id: jid,
                    kind: JobKind::Install,
                    payload: JobPayload::Package(id),
                    created_at: std::time::SystemTime::now(),
                    cancel: CancelToken::new(),
                });
            }
            Action::Remove(id) => {
                let jid = self.jid();
                let _ = self.tx_jobs.send(Job {
                    id: jid,
                    kind: JobKind::Remove,
                    payload: JobPayload::Package(id),
                    created_at: std::time::SystemTime::now(),
                    cancel: CancelToken::new(),
                });
            }
            Action::Progress(mut p) => {
                if let Some(ref mut l) = p.log {
                    l.push('\n');
                    s.progress_log.push_str(&l);
                    if s.progress_log.len() > MAX_LOG {
                        // Drain on a UTF-8 boundary to avoid panic.
                        let excess = s.progress_log.len() - MAX_LOG;
                        let mut drain_to = excess;
                        while drain_to < s.progress_log.len()
                            && !s.progress_log.is_char_boundary(drain_to)
                        {
                            drain_to += 1;
                        }
                        s.progress_log.drain(..drain_to.min(s.progress_log.len()));
                    }
                }
                if matches!(p.stage, Stage::Failed) && s.error.is_none() {
                    s.error = p
                        .log
                        .clone()
                        .and_then(|t| {
                            let t = t.trim().to_string();
                            if t.is_empty() { None } else { Some(t) }
                        })
                        .or_else(|| Some("operation failed".into()));
                }
            }
            Action::Event(e) => match e {
                Event::SearchResults { items, .. } => {
                    s.in_upgrades_view = false;
                    let q = s.query.to_lowercase();
                    let mut v = items
                        .into_iter()
                        .filter(|x| {
                            if q.is_empty() {
                                true
                            } else {
                                let name = x.id.name.to_lowercase();
                                let desc = x.description.to_lowercase();
                                name.contains(&q) || desc.contains(&q)
                            }
                        })
                        // Existing filters
                        .filter(|x| {
                            (s.filter_repo && x.id.source == Source::Repo)
                                || (s.filter_aur && x.id.source == Source::Aur)
                        })
                        .filter(|x| {
                            if s.filter_installed {
                                x.installed
                            } else {
                                true
                            }
                        })
                        .collect::<Vec<_>>();
                    // Sorting as before
                    match s.sort {
                        SortMode::NameAsc => v.sort_by(|a, b| a.id.name.cmp(&b.id.name)),
                        SortMode::NameDesc => v.sort_by(|a, b| b.id.name.cmp(&a.id.name)),
                        SortMode::Popularity => {
                            v.sort_by(|a, b| b.popular.unwrap_or(0).cmp(&a.popular.unwrap_or(0)))
                        }
                    }
                    s.results = v;
                    if let Some(sel) = &s.selected {
                        if !s.results.iter().any(|r| r.id == *sel) {
                            s.selected = None;
                        }
                    }
                }
                Event::Upgrades { items } => {
                    s.in_upgrades_view = true;
                    // Show upgrades in the same left pane, honoring filters/sort
                    let mut v = items
                        .into_iter()
                        .filter(|x| {
                            (s.filter_repo && x.id.source == Source::Repo)
                                || (s.filter_aur && x.id.source == Source::Aur)
                        })
                        .filter(|x| {
                            if s.filter_installed {
                                x.installed
                            } else {
                                true
                            }
                        })
                        .collect::<Vec<_>>();
                    match s.sort {
                        SortMode::NameAsc => v.sort_by(|a, b| a.id.name.cmp(&b.id.name)),
                        SortMode::NameDesc => v.sort_by(|a, b| b.id.name.cmp(&a.id.name)),
                        SortMode::Popularity => {
                            v.sort_by(|a, b| b.popular.unwrap_or(0).cmp(&a.popular.unwrap_or(0)))
                        }
                    }
                    s.results = v;
                    s.selected = None;
                }
                Event::Details { .. } => { /* not shown in v1 */ }
                Event::SystemChanged => {
                    // Decide what to refresh based on current UI mode.
                    if s.in_upgrades_view {
                        let id = self.jid();
                        let _ = self.tx_jobs.send(Job {
                            id,
                            kind: JobKind::Upgrades,
                            payload: JobPayload::None,
                            created_at: std::time::SystemTime::now(),
                            cancel: CancelToken::new(),
                        });
                    } else if !s.query.trim().is_empty() {
                        let id = self.jid();
                        let q = s.query.clone();
                        let _ = self.tx_jobs.send(Job {
                            id,
                            kind: JobKind::Search,
                            payload: JobPayload::Query(q),
                            created_at: std::time::SystemTime::now(),
                            cancel: CancelToken::new(),
                        });
                    }
                }
            },
            Action::ClearError => s.error = None,
            Action::Select(id) => s.selected = Some(id),
            Action::ClearSelection => s.selected = None,
            Action::ToggleFilterRepo => s.filter_repo = !s.filter_repo,
            Action::ToggleFilterAur => s.filter_aur = !s.filter_aur,
            Action::ToggleFilterInstalled => s.filter_installed = !s.filter_installed,
            Action::SetSort(m) => s.sort = m,
            Action::ToggleLog => s.log_expanded = !s.log_expanded,
        }
        self.state.set(s);
    }
}
