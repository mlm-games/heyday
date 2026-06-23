use domain::*;
use libflatpak::{
    gio::Cancellable,
    glib,
    prelude::*,
    Installation, Transaction,
};
use std::{
    cell::Cell,
    process::Command,
    rc::Rc,
};

pub struct FlatpakBackend {
    user: bool,
}

impl FlatpakBackend {
    pub fn new(user: bool) -> Self {
        glib::set_application_name("soredowe");
        Self { user }
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

    fn summary_from_ref<R>(r: &R) -> Option<PackageSummary>
    where
        R: RefExt + InstalledRefExt,
    {
        let name: String = r.name()?.to_string();
        let origin: String = r.origin().unwrap_or_default().to_string();
        let version: String = r.appdata_version().unwrap_or_default().to_string();
        let description: String = r
            .appdata_name()
            .unwrap_or_else(|| name.clone().into())
            .to_string();
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
        let output = Command::new("flatpak")
            .args([
                "search",
                "--columns=application,version,description,origin",
                "--",
                q,
            ])
            .output()
            .map_err(|e| Error::Flatpak(format!("flatpak search failed: {e}")))?;

        if !output.status.success() {
            return Ok(vec![]);
        }

        let installed: std::collections::HashSet<String> = self
            .installed(_cancel)
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.id.name)
            .collect();

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut results = Vec::new();

        for line in stdout.lines().skip(1) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut cols = line.split('\t');
            let name = match cols.next() {
                Some(n) => n.trim(),
                None => continue,
            };
            if name.is_empty() {
                continue;
            }
            let version = cols.next().unwrap_or("").trim();
            let description = cols.next().unwrap_or("").trim();
            let origin = cols.next().unwrap_or("").trim();

            results.push(PackageSummary {
                id: PackageId {
                    name: name.to_string(),
                    source: Source::Flatpak,
                    repo: if origin.is_empty() {
                        None
                    } else {
                        Some(origin.to_string())
                    },
                },
                version: version.to_string(),
                description: description.to_string(),
                installed: installed.contains(name),
                popular: None,
                last_updated: None,
            });
        }

        Ok(results)
    }

    fn details(
        &self,
        id: &PackageId,
        _sink: &ProgressSink,
        _cancel: &CancelToken,
    ) -> Result<PackageDetails> {
        let output = Command::new("flatpak")
            .args(["info", "--", &id.name])
            .output()
            .map_err(|e| Error::Flatpak(format!("flatpak info failed: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut version = String::new();
        let mut description = String::new();
        let mut homepage = None;

        for line in stdout.lines() {
            let line = line.trim();
            if let Some(v) = line.strip_prefix("Version:") {
                version = v.trim().to_string();
            } else if let Some(d) = line.strip_prefix("Description:") {
                description = d.trim().to_string();
            } else if let Some(h) = line.strip_prefix("Homepage:") {
                let h = h.trim();
                if !h.is_empty() && h != "-" {
                    homepage = Some(h.to_string());
                }
            } else if version.is_empty() && line.starts_with("Version:") {
                version = line
                    .strip_prefix("Version:")
                    .unwrap_or("")
                    .trim()
                    .to_string();
            }
        }

        Ok(PackageDetails {
            summary: PackageSummary {
                id: id.clone(),
                version,
                description,
                installed: true,
                popular: None,
                last_updated: None,
            },
            depends: vec![],
            opt_depends: vec![],
            homepage,
            maintainer: None,
            size_install: None,
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
            if let Some(s) = Self::summary_from_ref(r) {
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
            if let Some(s) = Self::summary_from_ref(r) {
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
