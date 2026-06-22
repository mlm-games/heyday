use crossbeam_channel as chan;
use domain::*;
use repose_core::locals::with_theme;
use repose_core::signal::signal;
use repose_core::*;
use repose_material::material3;
use repose_ui::overlay::{SnackbarController, SnackbarRequest};
use std::rc::Rc;
use std::sync::atomic::AtomicU64;

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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppState {
    pub query: String,

    /// Raw results from the last search/upgrades event (unfiltered).
    pub raw_results: Vec<PackageSummary>,
    /// Filtered + sorted view shown in the list.
    pub results: Vec<PackageSummary>,

    pub selected: Option<PackageId>,
    /// Full details for the selected package (fetched lazily).
    pub detail: Option<PackageDetails>,

    pub filter_repo: bool,
    pub filter_aur: bool,
    pub filter_installed: bool,
    pub sort: SortMode,

    pub progress_log: String,
    pub log_expanded: bool,
    pub in_upgrades_view: bool,

    /// Current operation stage, if any. Set to None when idle/finished/failed.
    pub active_stage: Option<Stage>,
    /// Current progress fraction (0.0–1.0), if known.
    pub progress_pct: Option<f32>,
}

impl AppState {
    /// Recompute `results` from `raw_results` + current filters/sort.
    pub fn refilter(&mut self) {
        let mut v: Vec<PackageSummary> = self
            .raw_results
            .iter()
            .cloned()
            .filter(|x| {
                (self.filter_repo && x.id.source == Source::Repo)
                    || (self.filter_aur && x.id.source == Source::Aur)
            })
            .filter(|x| !self.filter_installed || x.installed)
            .collect();

        match self.sort {
            SortMode::NameAsc => v.sort_by(|a, b| a.id.name.cmp(&b.id.name)),
            SortMode::NameDesc => v.sort_by(|a, b| b.id.name.cmp(&a.id.name)),
            SortMode::Popularity => {
                v.sort_by(|a, b| b.popular.unwrap_or(0).cmp(&a.popular.unwrap_or(0)))
            }
        }
        self.results = v;
        self.prune_selection();
    }

    pub fn append_log(&mut self, line: &str) {
        self.progress_log.push_str(line);
        self.progress_log.push('\n');
        if self.progress_log.len() > MAX_LOG {
            let excess = self.progress_log.len() - MAX_LOG;
            let mut drain_to = excess;
            while drain_to < self.progress_log.len()
                && !self.progress_log.is_char_boundary(drain_to)
            {
                drain_to += 1;
            }
            self.progress_log
                .drain(..drain_to.min(self.progress_log.len()));
        }
    }

    fn prune_selection(&mut self) {
        if let Some(sel) = &self.selected {
            if !self.results.iter().any(|r| r.id == *sel) {
                self.selected = None;
                self.detail = None;
            }
        }
    }
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
    Select(PackageId),
    ClearSelection,
    ToggleFilterRepo,
    ToggleFilterAur,
    ToggleFilterInstalled,
    SetSort(SortMode),
    ToggleLog,
}

pub struct Store {
    pub state: Signal<AppState>,
    tx_jobs: chan::Sender<domain::Job>,
    next_id: AtomicU64,
    pub snackbar: Option<SnackbarController>,
}

impl Store {
    pub fn new(tx_jobs: chan::Sender<domain::Job>, snackbar: Option<SnackbarController>) -> Self {
        let s = AppState {
            filter_repo: true,
            filter_aur: true,
            sort: SortMode::default(),
            ..Default::default()
        };
        Self {
            state: signal(s),
            tx_jobs,
            next_id: std::sync::atomic::AtomicU64::new(1),
            snackbar,
        }
    }

    fn jid(&self) -> u64 {
        self.next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    fn send_job(&self, kind: JobKind, payload: JobPayload) {
        let _ = self.tx_jobs.send(Job {
            id: self.jid(),
            kind,
            payload,
            created_at: std::time::SystemTime::now(),
            cancel: CancelToken::new(),
        });
    }

    fn send_search(&self, q: &str) {
        self.send_job(JobKind::Search, JobPayload::Query(q.to_string()));
    }

    fn send_upgrades(&self) {
        self.send_job(JobKind::Upgrades, JobPayload::None);
    }

    fn send_details(&self, id: &PackageId) {
        self.send_job(JobKind::Details, JobPayload::Package(id.clone()));
    }

    fn refresh_current_view(&self, s: &AppState) {
        if s.in_upgrades_view {
            self.send_upgrades();
        } else if !s.query.trim().is_empty() {
            self.send_search(&s.query);
        }
    }

    pub fn show_snackbar(&self, msg: String) {
        if let Some(ref snackbar) = self.snackbar {
            let mut snackbar_theme = theme();
            snackbar_theme.colors.surface_variant = snackbar_theme.error_container;
            snackbar_theme.colors.on_surface = snackbar_theme.on_error_container;
            snackbar_theme.colors.primary = snackbar_theme.on_error_container;
            snackbar_theme.colors.outline_variant = snackbar_theme.error_container;
            let msg = msg.clone();
            let request = SnackbarRequest {
                message: msg.clone(),
                action: None,
                duration_ms: 6000,
            builder: Rc::new(move || {
                with_theme(snackbar_theme, || {
                    material3::Snackbar(
                        msg.clone(),
                        None,
                        Modifier::new()
                            .absolute()
                            .offset(Some(16.0), None, Some(16.0), Some(16.0)),
                        material3::SnackbarConfig {
                            container_color: snackbar_theme.error_container,
                            content_color: snackbar_theme.on_error_container,
                            ..Default::default()
                        },
                    )
                })
            }),
            };
            snackbar.show(request);
        }
    }

    pub fn dispatch(&self, a: Action) {
        let mut s = self.state.get();
        match a {
            Action::SetQuery(q) => s.query = q,

            Action::Search => {
                s.in_upgrades_view = false;
                s.detail = None;
                let q = s.query.trim().to_string();
                self.send_search(&q);
                if q.is_empty() {
                    s.raw_results.clear();
                    s.results.clear();
                    s.selected = None;
                }
            }

            Action::Refresh => {
                self.send_job(JobKind::Refresh, JobPayload::None);
                self.refresh_current_view(&s);
            }

            Action::Upgrades => {
                s.in_upgrades_view = true;
                s.detail = None;
                self.send_upgrades();
            }

            Action::UpgradeAll => {
                self.send_job(JobKind::UpgradeAll, JobPayload::None);
            }

            Action::Upgrade(id) => {
                self.send_job(JobKind::Upgrade, JobPayload::Package(id));
            }

            Action::Install(id) => {
                self.send_job(JobKind::Install, JobPayload::Package(id));
            }

            Action::Remove(id) => {
                self.send_job(JobKind::Remove, JobPayload::Package(id));
            }

            Action::Progress(mut p) => {
                let log = p.log.take();
                if let Some(ref l) = log {
                    s.append_log(l);
                }

                match p.stage {
                    Stage::Finished | Stage::Failed => {
                        s.active_stage = None;
                        s.progress_pct = None;
                    }
                    _ => {
                        s.active_stage = Some(p.stage);
                        s.progress_pct = p.percent.or_else(|| {
                            p.bytes.and_then(|(cur, tot)| {
                                if tot > 0 {
                                    Some(cur as f32 / tot as f32)
                                } else {
                                    None
                                }
                            })
                        });
                    }
                }

                if matches!(p.stage, Stage::Failed) {
                    let msg = log
                        .clone()
                        .and_then(|t| {
                            let t = t.trim().to_string();
                            if t.is_empty() { None } else { Some(t) }
                        })
                        .or_else(|| Some("operation failed".into()));
                    if let Some(m) = msg {
                        self.show_snackbar(m);
                    }
                }
            }

            Action::Event(e) => match e {
                Event::SearchResults { items, .. } => {
                    s.in_upgrades_view = false;
                    let q = s.query.to_lowercase();
                    s.raw_results = items
                        .into_iter()
                        .filter(|x| {
                            q.is_empty()
                                || x.id.name.to_lowercase().contains(&q)
                                || x.description.to_lowercase().contains(&q)
                        })
                        .collect();
                    s.refilter();
                }
                Event::Upgrades { items } => {
                    s.in_upgrades_view = true;
                    s.raw_results = items;
                    s.refilter();
                    s.selected = None;
                    s.detail = None;
                }
                Event::Details { item } => {
                    // Only accept if it matches the current selection.
                    if s.selected.as_ref() == Some(&item.summary.id) {
                        s.detail = Some(item);
                    }
                }
                Event::SystemChanged => {
                    self.refresh_current_view(&s);
                }
            },

            Action::Select(id) => {
                if s.selected.as_ref() != Some(&id) {
                    s.detail = None; // clear stale detail
                    self.send_details(&id);
                }
                s.selected = Some(id);
            }

            Action::ClearSelection => {
                s.selected = None;
                s.detail = None;
            }

            Action::ToggleFilterRepo => {
                s.filter_repo = !s.filter_repo;
                s.refilter();
            }
            Action::ToggleFilterAur => {
                s.filter_aur = !s.filter_aur;
                s.refilter();
            }
            Action::ToggleFilterInstalled => {
                s.filter_installed = !s.filter_installed;
                s.refilter();
            }
            Action::SetSort(m) => {
                s.sort = m;
                s.refilter();
            }

            Action::ToggleLog => s.log_expanded = !s.log_expanded,
        }
        self.state.set(s);
    }
}
