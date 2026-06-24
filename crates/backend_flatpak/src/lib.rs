use domain::*;
use libflatpak::{Installation, Transaction, gio::Cancellable, glib, prelude::*};
use std::{cell::Cell, collections::HashMap, fs, rc::Rc, sync::Mutex};

use domain::appstream::AppstreamMeta;

/// Find and parse all appstream XML files for a flatpak installation.
fn load_appstream_cache(inst: &Installation) -> HashMap<String, AppstreamMeta> {
    let remotes = match inst.list_remotes(Cancellable::NONE) {
        Ok(r) => r,
        Err(_) => return HashMap::new(),
    };

    let mut all = HashMap::new();
    for remote in &remotes {
        let dir = match remote.appstream_dir(None).and_then(|x| x.path()) {
            Some(p) => p,
            None => continue,
        };
        let (file, gzipped) = if dir.join("appstream.xml.gz").is_file() {
            (dir.join("appstream.xml.gz"), true)
        } else if dir.join("appstream.xml").is_file() {
            (dir.join("appstream.xml"), false)
        } else {
            continue;
        };
        let reader = match fs::File::open(&file) {
            Ok(f) => f,
            Err(e) => {
                log::warn!("failed to open appstream file {:?}: {e}", file);
                continue;
            }
        };
        log::info!("loading appstream cache from {:?}", file);
        let apps = if gzipped {
            domain::appstream::parse_appstream_xml_gz(reader)
        } else {
            domain::appstream::parse_appstream_xml(reader)
        };
        all.extend(apps);
    }
    all
}

pub struct FlatpakBackend {
    user_modes: Vec<bool>,
    app_cache: Mutex<HashMap<String, AppstreamMeta>>,
}

impl FlatpakBackend {
    fn open_installation(user: bool) -> Option<Installation> {
        let inst = if user {
            Installation::new_user(Cancellable::NONE)
        } else {
            Installation::new_system(Cancellable::NONE)
        };
        inst.ok()
    }

    pub fn new(_user_request: bool) -> Result<Self> {
        glib::set_application_name("soredowe");

        let user_modes: Vec<bool> = [true, false]
            .into_iter()
            .filter(|u| Self::open_installation(*u).is_some())
            .collect();

        if user_modes.is_empty() {
            return Err(Error::Flatpak("no flatpak installation found".into()));
        }

        let cache = {
            let mut merged = HashMap::new();
            for u in &user_modes {
                if let Some(inst) = Self::open_installation(*u) {
                    let _ = inst.drop_caches(Cancellable::NONE);
                    merged.extend(load_appstream_cache(&inst));
                }
            }
            merged
        };

        Ok(Self {
            user_modes,
            app_cache: Mutex::new(cache),
        })
    }

    fn with_each_installation(&self, f: &mut dyn FnMut(&Installation)) {
        for u in &self.user_modes {
            if let Some(inst) = Self::open_installation(*u) {
                f(&inst);
            }
        }
    }

    fn find_installed_ref(&self, name: &str) -> Option<(String, bool)> {
        for u in &self.user_modes {
            let Some(inst) = Self::open_installation(*u) else {
                continue;
            };
            let Ok(refs) = inst.list_installed_refs(Cancellable::NONE) else {
                continue;
            };
            for r in &refs {
                if r.name().as_deref() == Some(name) {
                    if let Some(ref_str) = r.format_ref().map(|s| s.to_string()) {
                        return Some((ref_str, *u));
                    }
                }
            }
        }
        None
    }

    fn summary_from_ref<R>(
        r: &R,
        app_cache: &HashMap<String, AppstreamMeta>,
    ) -> Option<PackageSummary>
    where
        R: RefExt + InstalledRefExt,
    {
        let name = r.name()?.to_string();
        let origin = r.origin().unwrap_or_default().to_string();
        let version: String = r.appdata_version().unwrap_or_default().to_string();
        let app_info = app_cache.get(&name);
        let description = app_info
            .map(|a| a.summary.clone())
            .or_else(|| r.appdata_name().map(|s| s.to_string()))
            .unwrap_or_default();
        Some(PackageSummary {
            id: PackageId {
                name,
                source: Source::Flatpak,
                repo: Some(origin),
            },
            version,
            description,
            installed: true,
            popular: None,
            last_updated: None,
        })
    }

    /// Construct a flatpak ref string.
    /// If `branch` is empty, defaults to `"stable"`.
    fn name_to_ref(name: &str, branch: &str) -> String {
        let arch = std::env::consts::ARCH;
        let br = if branch.is_empty() { "stable" } else { branch };
        format!("app/{name}/{arch}/{br}")
    }

    fn with_transaction_on(
        &self,
        user: bool,
        stage: Stage,
        setup: impl FnOnce(&Transaction) -> std::result::Result<(), glib::Error>,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> Result<()> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let inst = Self::open_installation(user)
            .ok_or_else(|| Error::Flatpak("installation unavailable".into()))?;
        let tx = Transaction::for_installation(&inst, Cancellable::NONE)
            .map_err(|e| Error::Flatpak(e.to_string()))?;

        let (tx_prog_inner, rx_prog_inner) = std::sync::mpsc::channel();
        let total_ops = Rc::new(Cell::new(0u64));
        let started_ops = Rc::new(Cell::new(0u64));

        tx.connect_ready({
            let total_ops = total_ops.clone();
            move |tx: &Transaction| {
                total_ops.set(tx.operations().len() as u64);
                true
            }
        });

        tx.connect_new_operation({
            let total_ops = total_ops.clone();
            let started_ops = started_ops.clone();
            let tx_prog = tx_prog_inner.clone();
            move |_: &Transaction, _: &libflatpak::TransactionOperation, progress| {
                let current_op = started_ops.get();
                started_ops.set(current_op + 1);
                let total = total_ops.get().max(current_op + 1);
                let per_op = if total > 0 {
                    100.0 / total as f32
                } else {
                    100.0
                };
                let tx_prog = tx_prog.clone();
                progress.connect_changed(move |p| {
                    let op_progress = (p.progress() as f32) / 100.0;
                    let total_progress = ((current_op as f32) + op_progress) * per_op;
                    let _ = tx_prog.send(total_progress / 100.0);
                });
            }
        });

        setup(&tx).map_err(|e| Error::Flatpak(e.to_string()))?;

        let sink = sink.clone();
        let jh = std::thread::spawn(move || {
            while let Ok(pct) = rx_prog_inner.recv() {
                let _ = sink.send(Progress {
                    job_id: 0,
                    stage,
                    percent: Some(pct),
                    bytes: None,
                    log: None,
                    warning: false,
                });
            }
        });

        let result = tx
            .run(Cancellable::NONE)
            .map_err(|e| Error::Flatpak(e.to_string()));
        drop(tx_prog_inner);
        let _ = jh.join();
        result
    }

    fn with_transaction(
        &self,
        stage: Stage,
        setup: impl FnOnce(&Transaction) -> std::result::Result<(), glib::Error>,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> Result<()> {
        // Use the first available installation
        let user = *self.user_modes.first().unwrap_or(&true);
        self.with_transaction_on(user, stage, setup, sink, cancel)
    }
}

impl PackageBackend for FlatpakBackend {
    fn name(&self) -> &'static str {
        "flatpak"
    }

    fn refresh(&self, sink: &ProgressSink, _cancel: &CancelToken) -> Result<()> {
        for u in &self.user_modes {
            let Some(inst) = Self::open_installation(*u) else {
                continue;
            };
            let Ok(remotes) = inst.list_remotes(Cancellable::NONE) else {
                continue;
            };
            for remote in &remotes {
                if let Some(name) = remote.name() {
                    let _ = sink.send(Progress {
                        job_id: 0,
                        stage: Stage::Searching,
                        percent: None,
                        bytes: None,
                        log: Some(format!("updating flatpak remote: {name}")),
                        warning: false,
                    });
                    if let Err(e) = inst.update_remote_sync(&name, Cancellable::NONE) {
                        log::warn!("failed to update remote {name}: {e}");
                    }
                    if let Err(e) = inst.update_appstream_sync(&name, None, Cancellable::NONE) {
                        log::warn!("failed to update appstream for {name}: {e}");
                    }
                }
            }
        }
        // Reload cache from all installations
        let mut merged = HashMap::new();
        for u in &self.user_modes {
            if let Some(inst) = Self::open_installation(*u) {
                let _ = inst.drop_caches(Cancellable::NONE);
                merged.extend(load_appstream_cache(&inst));
            }
        }
        *self.app_cache.lock().unwrap() = merged;
        Ok(())
    }

    fn search(
        &self,
        q: &str,
        _sink: &ProgressSink,
        _cancel: &CancelToken,
    ) -> Result<Vec<PackageSummary>> {
        let q = q.to_lowercase();
        if q.len() < 2 {
            return Ok(vec![]);
        }

        let installed: std::collections::HashSet<String> = self
            .installed(_cancel)
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.id.name)
            .collect();

        let cache = self.app_cache.lock().unwrap();
        let mut results: Vec<PackageSummary> = Vec::new();
        for (app_id, info) in cache.iter() {
            let id_lower = app_id.to_lowercase();
            let name_lower = info.name.to_lowercase();
            let summary_lower = info.summary.to_lowercase();
            if id_lower.contains(&q) || name_lower.contains(&q) || summary_lower.contains(&q) {
                results.push(PackageSummary {
                    id: PackageId {
                        name: app_id.clone(),
                        source: Source::Flatpak,
                        repo: Some("flathub".to_string()),
                    },
                    version: info.version.clone().unwrap_or_default(),
                    description: info.summary.clone(),
                    installed: installed.contains(app_id.as_str()),
                    popular: None,
                    last_updated: None,
                });
            }
        }

        results.sort_by(|a, b| a.id.name.cmp(&b.id.name));
        Ok(results)
    }

    fn details(
        &self,
        id: &PackageId,
        _sink: &ProgressSink,
        _cancel: &CancelToken,
    ) -> Result<PackageDetails> {
        let cache = self.app_cache.lock().unwrap();
        let info = cache.get(&id.name);

        let mut version = String::new();
        let mut size_install = None;
        let mut is_installed = false;
        'outer: for u in &self.user_modes {
            let Some(inst) = Self::open_installation(*u) else {
                continue;
            };
            let _ = inst.drop_caches(Cancellable::NONE);
            if let Ok(refs) = inst.list_installed_refs(Cancellable::NONE) {
                for r in &refs {
                    if r.name().as_deref() == Some(&id.name) {
                        is_installed = true;
                        version = r.appdata_version().unwrap_or_default().to_string();
                        let install_size = r.installed_size();
                        if install_size > 0 {
                            size_install = Some(install_size);
                        }
                        break 'outer;
                    }
                }
            }
        }
        if version.is_empty() {
            version = info.and_then(|i| i.version.clone()).unwrap_or_default();
        }

        Ok(PackageDetails {
            summary: PackageSummary {
                id: id.clone(),
                version,
                description: info.as_ref().map(|i| i.summary.clone()).unwrap_or_default(),
                installed: is_installed,
                popular: None,
                last_updated: None,
            },
            description: info.as_ref().and_then(|i| {
                let d = i.description.trim();
                if d.is_empty() {
                    None
                } else {
                    Some(d.to_string())
                }
            }),
            depends: vec![],
            opt_depends: vec![],
            homepage: info.as_ref().and_then(|i| i.homepage.clone()),
            license: info.as_ref().and_then(|i| i.license.clone()),
            maintainer: None,
            developer: info.as_ref().and_then(|i| i.developer.clone()),
            size_install,
            size_download: None,
        })
    }

    fn installed(&self, _cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        let cache = self.app_cache.lock().unwrap();
        let mut results = Vec::new();
        for u in &self.user_modes {
            let Some(inst) = Self::open_installation(*u) else {
                continue;
            };
            let _ = inst.drop_caches(Cancellable::NONE);
            let Ok(refs) = inst.list_installed_refs(Cancellable::NONE) else {
                continue;
            };
            for r in &refs {
                if let Some(s) = Self::summary_from_ref(r, &*cache) {
                    results.push(s);
                }
            }
        }
        Ok(results)
    }

    fn updates(&self, _cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        let cache = self.app_cache.lock().unwrap();
        let mut results = Vec::new();
        for u in &self.user_modes {
            let Some(inst) = Self::open_installation(*u) else {
                continue;
            };
            let _ = inst.drop_caches(Cancellable::NONE);
            let Ok(refs) = inst.list_installed_refs_for_update(Cancellable::NONE) else {
                continue;
            };
            for r in &refs {
                if let Some(s) = Self::summary_from_ref(r, &*cache) {
                    results.push(s);
                }
            }
        }
        Ok(results)
    }

    fn operation(
        &self,
        op: &Operation,
        sink: &ProgressSink,
        cancel: &CancelToken,
        _progress: Box<dyn FnMut(f32) + Send + 'static>,
    ) -> Result<()> {
        match &op.kind {
            OperationKind::Install => {
                for pid in &op.package_ids {
                    self.install(pid, sink, cancel)?;
                }
                Ok(())
            }
            OperationKind::Remove { .. } => {
                for pid in &op.package_ids {
                    self.remove(pid, sink, cancel)?;
                }
                Ok(())
            }
            OperationKind::Update => {
                for pid in &op.package_ids {
                    self.upgrade(pid, sink, cancel)?;
                }
                Ok(())
            }
            OperationKind::Refresh => self.refresh(sink, cancel),
        }
    }

    fn install(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let (remote, branch) = id
            .repo
            .as_deref()
            .map(|r| match r.split_once(':') {
                Some((rem, br)) => (rem.to_string(), br.to_string()),
                None => (r.to_string(), String::new()),
            })
            .unwrap_or_else(|| ("flathub".to_string(), String::new()));
        let ref_str = Self::name_to_ref(&id.name, &branch);
        // Install to the first available installation (user-preferred)
        let user = *self.user_modes.first().unwrap_or(&true);
        self.with_transaction_on(
            user,
            Stage::Installing,
            |tx: &Transaction| {
                tx.add_install(&remote, &ref_str, &[])?;
                Ok(())
            },
            sink,
            cancel,
        )
    }

    fn remove(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let (ref_str, user) = self
            .find_installed_ref(&id.name)
            .ok_or_else(|| Error::Flatpak(format!("{} not installed", id.name)))?;
        self.with_transaction_on(
            user,
            Stage::Removing,
            move |tx: &Transaction| {
                tx.add_uninstall(&ref_str)?;
                Ok(())
            },
            sink,
            cancel,
        )
    }

    fn upgrade(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let (ref_str, user) = self
            .find_installed_ref(&id.name)
            .ok_or_else(|| Error::Flatpak(format!("{} not installed", id.name)))?;
        self.with_transaction_on(
            user,
            Stage::Installing,
            move |tx: &Transaction| {
                tx.add_update(&ref_str, &[], None)?;
                Ok(())
            },
            sink,
            cancel,
        )
    }

    fn upgrade_all(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let updates = self.updates(cancel)?;
        if updates.is_empty() {
            return Ok(());
        }
        let mut refs: Vec<(String, bool)> = Vec::new();
        for u in &self.user_modes {
            let Some(inst) = Self::open_installation(*u) else {
                continue;
            };
            let _ = inst.drop_caches(Cancellable::NONE);
            let Ok(urefs) = inst.list_installed_refs_for_update(Cancellable::NONE) else {
                continue;
            };
            for r in &urefs {
                if let Some(ref_str) = r.format_ref().map(|s| s.to_string()) {
                    refs.push((ref_str, *u));
                }
            }
        }
        if refs.is_empty() {
            return Ok(());
        }
        // Group by installation type
        for u in &self.user_modes {
            let batch: Vec<&str> = refs
                .iter()
                .filter(|(_, mode)| mode == u)
                .map(|(r, _)| r.as_str())
                .collect();
            if batch.is_empty() {
                continue;
            }
            self.with_transaction_on(
                *u,
                Stage::Installing,
                |tx: &Transaction| {
                    for r in batch {
                        tx.add_update(r, &[], None)?;
                    }
                    Ok(())
                },
                sink,
                cancel,
            )?;
        }
        Ok(())
    }
}
