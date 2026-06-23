use domain::*;
use libflatpak::{
    gio::Cancellable,
    glib, prelude::*,
    Installation, Transaction,
};
use std::{
    cell::Cell,
    collections::HashMap,
    fs,
    rc::Rc,
};

use domain::appstream::AppstreamMeta;

/// Find and parse all appstream XML files for a flatpak installation.
fn load_appstream_cache(user: bool) -> HashMap<String, AppstreamMeta> {
    let inst = if user {
        Installation::new_user(Cancellable::NONE)
    } else {
        Installation::new_system(Cancellable::NONE)
    };
    let inst = match inst {
        Ok(i) => i,
        Err(_) => return HashMap::new(),
    };

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
    user: bool,
    app_cache: HashMap<String, AppstreamMeta>,
}

impl FlatpakBackend {
    pub fn new(user: bool) -> Self {
        glib::set_application_name("soredowe");
        let cache = load_appstream_cache(user);
        Self {
            user,
            app_cache: cache,
        }
    }

    fn installation(&self) -> std::result::Result<Installation, Error> {
        let inst = if self.user {
            Installation::new_user(Cancellable::NONE)
        } else {
            Installation::new_system(Cancellable::NONE)
        };
        inst.map_err(|e| Error::Flatpak(e.to_string()))
    }

    fn installed_ref_str(&self, name: &str) -> Option<String> {
        let inst = self.installation().ok()?;
        let refs: Vec<libflatpak::InstalledRef> =
            inst.list_installed_refs(Cancellable::NONE).ok()?;
        for r in &refs {
            if r.name().as_deref() == Some(name) {
                return r.format_ref().map(|s| s.to_string());
            }
        }
        None
    }

    fn summary_from_ref<R>(r: &R, app_cache: &HashMap<String, AppstreamMeta>) -> Option<PackageSummary>
    where
        R: RefExt + InstalledRefExt,
    {
        let name = r.name()?.to_string();
        let origin = r.origin().unwrap_or_default().to_string();
        let version: String = r.appdata_version().unwrap_or_default().to_string();
        let app_info = app_cache.get(&name);
        let description = app_info
            .map(|a| a.summary.clone())
            .or_else(|| {
                r.appdata_name()
                    .map(|s| s.to_string())
            })
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

    fn name_to_ref(name: &str) -> String {
        format!("app/{name}/x86_64/stable")
    }

    fn with_transaction(
        &self,
        setup: impl FnOnce(&Transaction) -> std::result::Result<(), glib::Error>,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> Result<()> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let inst = self.installation()?;
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
                    stage: Stage::Installing,
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
}

impl PackageBackend for FlatpakBackend {
    fn name(&self) -> &'static str {
        "flatpak"
    }

    fn refresh(&self, _sink: &ProgressSink, _cancel: &CancelToken) -> Result<()> {
        let inst = self.installation()?;
        let remotes: Vec<libflatpak::Remote> = inst
            .list_remotes(Cancellable::NONE)
            .map_err(|e| Error::Flatpak(e.to_string()))?;
        for remote in &remotes {
            if let Some(name) = remote.name() {
                log::info!("updating flatpak remote {name}");
                let _ = inst.update_remote_sync(&name, Cancellable::NONE);
                let _ = inst.update_appstream_sync(&name, None, Cancellable::NONE);
            }
        }
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

        let mut results: Vec<PackageSummary> = Vec::new();
        for (app_id, info) in &self.app_cache {
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
        let info = self.app_cache.get(&id.name);

    let mut version = String::new();
    let mut size_install = None;
    let mut is_installed = false;
    if let Ok(inst) = self.installation() {
        if let Ok(refs) = inst.list_installed_refs(Cancellable::NONE) {
            for r in &refs {
                if r.name().as_deref() == Some(&id.name) {
                    is_installed = true;
                    version = r.appdata_version().unwrap_or_default().to_string();
                    let install_size = r.installed_size();
                    if install_size > 0 {
                        size_install = Some(install_size);
                    }
                    break;
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
            description: info
                .as_ref()
                .map(|i| i.summary.clone())
                .unwrap_or_default(),
            installed: is_installed,
            popular: None,
            last_updated: None,
        },
            description: info.as_ref().and_then(|i| {
                let d = i.description.trim();
                if d.is_empty() { None } else { Some(d.to_string()) }
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
        let inst = self.installation()?;
        let refs: Vec<libflatpak::InstalledRef> = inst
            .list_installed_refs(Cancellable::NONE)
            .map_err(|e| Error::Flatpak(e.to_string()))?;
        let mut results = Vec::new();
        for r in &refs {
            if let Some(s) = Self::summary_from_ref(r, &self.app_cache) {
                results.push(s);
            }
        }
        Ok(results)
    }

    fn updates(&self, _cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        let inst = self.installation()?;
        let refs: Vec<libflatpak::InstalledRef> = inst
            .list_installed_refs_for_update(Cancellable::NONE)
            .map_err(|e| Error::Flatpak(e.to_string()))?;
        let mut results = Vec::new();
        for r in &refs {
            if let Some(s) = Self::summary_from_ref(r, &self.app_cache) {
                results.push(s);
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
        let remote = id.repo.as_deref().unwrap_or("flathub");
        let ref_str = Self::name_to_ref(&id.name);
        self.with_transaction(
            |tx: &Transaction| {
                tx.add_install(remote, &ref_str, &[])?;
                Ok(())
            },
            sink,
            cancel,
        )
    }

    fn remove(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let ref_str = self
            .installed_ref_str(&id.name)
            .ok_or_else(|| Error::Flatpak(format!("{} not installed", id.name)))?;
        self.with_transaction(
            move |tx: &Transaction| {
                tx.add_uninstall(&ref_str)?;
                Ok(())
            },
            sink,
            cancel,
        )
    }

    fn upgrade(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let ref_str = self
            .installed_ref_str(&id.name)
            .ok_or_else(|| Error::Flatpak(format!("{} not installed", id.name)))?;
        self.with_transaction(
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
        let refs: Vec<String> = updates
            .iter()
            .filter_map(|u| self.installed_ref_str(&u.id.name))
            .collect();
        if refs.is_empty() {
            return Ok(());
        }
        self.with_transaction(
            |tx: &Transaction| {
                for r in &refs {
                    tx.add_update(r, &[], None)?;
                }
                Ok(())
            },
            sink,
            cancel,
        )
    }
}
