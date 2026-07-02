use domain::*;
use packagekit_zbus::{
    package_kit::PackageKitProxyBlocking,
    transaction::TransactionProxyBlocking,
    zbus::{
        MatchRule,
        blocking::{Connection, MessageIterator},
        message::Type,
        zvariant::{OwnedValue, Value},
    },
};
use std::collections::HashMap;

#[repr(u64)]
enum Filter {
    None = 1 << 1,
    Installed = 1 << 2,
    NotInstalled = 1 << 3,
    Newest = 1 << 16,
    Arch = 1 << 18,
}

#[allow(dead_code)]
#[repr(u64)]
enum TxnFlag {
    OnlyTrusted = 1 << 1,
}

struct PkgDetail {
    _package_id: String,
    _summary: String,
    _description: String,
    _url: String,
}

struct PkgInfo {
    _info: u32,
    package_id: String,
    summary: String,
}

fn parse_pkg_id(package_id: &str) -> (String, String) {
    let mut parts = package_id.split(';');
    let name = parts.next().unwrap_or("").to_string();
    let version = parts.next().unwrap_or("").to_string();
    (name, version)
}

fn get_str(map: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    match map.get(key).map(|v| &**v) {
        Some(Value::Str(s)) => Some(s.to_string()),
        _ => None,
    }
}

fn collect(
    txn: TransactionProxyBlocking<'_>,
    cancel: &CancelToken,
    on_progress: &mut dyn FnMut(u32),
) -> std::result::Result<(Vec<PkgDetail>, Vec<PkgInfo>), Error> {
    let inner = txn.inner();
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface("org.freedesktop.PackageKit.Transaction")
        .map_err(|e| Error::PackageKit(e.to_string()))?
        .path(inner.path())
        .map_err(|e| Error::PackageKit(e.to_string()))?
        .build();
    let conn = inner.connection();
    let iter = MessageIterator::for_match_rule(rule, conn, None)
        .map_err(|e| Error::PackageKit(e.to_string()))?;

    let mut details = Vec::new();
    let mut packages = Vec::new();

    for result in iter {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let msg = result.map_err(|e| Error::PackageKit(e.to_string()))?;
        let header = msg.header();
        let Some(member) = header.member() else {
            continue;
        };
        match member.as_str() {
            "Details" => {
                let map: HashMap<String, OwnedValue> = msg
                    .body()
                    .deserialize()
                    .map_err(|e| Error::PackageKit(e.to_string()))?;
                let package_id = get_str(&map, "package-id").unwrap_or_default();
                let summary = get_str(&map, "summary").unwrap_or_default();
                let description = get_str(&map, "description").unwrap_or_default();
                let url = get_str(&map, "url").unwrap_or_default();
                details.push(PkgDetail {
                    _package_id: package_id,
                    _summary: summary,
                    _description: description,
                    _url: url,
                });
            }
            "ErrorCode" => {
                let (code, detail): (u32, String) = msg
                    .body()
                    .deserialize()
                    .map_err(|e| Error::PackageKit(e.to_string()))?;
                if code != 48 {
                    return Err(Error::PackageKit(format!("{detail} (code {code})")));
                }
            }
            "ItemProgress" => {
                let (_pid, _status, pct): (String, u32, u32) = msg
                    .body()
                    .deserialize()
                    .map_err(|e| Error::PackageKit(e.to_string()))?;
                on_progress(pct);
            }
            "Package" => {
                let (info, package_id, summary): (u32, String, String) =
                    msg.body()
                        .deserialize()
                        .map_err(|e| Error::PackageKit(e.to_string()))?;
                packages.push(PkgInfo {
                    _info: info,
                    package_id,
                    summary,
                });
            }
            "Finished" => break,
            _ => {
                log::debug!("unhandled PK signal: {member}");
            }
        }
    }

    Ok((details, packages))
}

/// Build a closure that sends progress to a ProgressSink.
fn send_progress(sink: &ProgressSink, stage: Stage) -> impl FnMut(u32) + '_ {
    let sink = (*sink).clone();
    move |pct| {
        let _ = sink.send(Progress {
            job_id: 0,
            stage,
            percent: Some(pct as f32 / 100.0),
            bytes: None,
            log: None,
            warning: false,
        });
    }
}

pub struct PackageKitBackend {
    connection: Connection,
}

impl PackageKitBackend {
    pub fn new() -> std::result::Result<Self, Error> {
        let connection = Connection::system().map_err(|e| Error::Internal(e.to_string()))?;
        Ok(Self { connection })
    }

    fn txn(&self) -> std::result::Result<TransactionProxyBlocking<'_>, Error> {
        let pk = PackageKitProxyBlocking::new(&self.connection)
            .map_err(|e| Error::Internal(e.to_string()))?;
        let path = pk
            .create_transaction()
            .map_err(|e| Error::PackageKit(e.to_string()))?;
        TransactionProxyBlocking::builder(&self.connection)
            .destination("org.freedesktop.PackageKit")
            .map_err(|e| Error::Internal(e.to_string()))?
            .path(path)
            .map_err(|e| Error::Internal(e.to_string()))?
            .build()
            .map_err(|e| Error::PackageKit(e.to_string()))
    }
}

impl PackageBackend for PackageKitBackend {
    fn name(&self) -> &'static str {
        "packagekit"
    }

    fn group(&self) -> &'static str {
        "repo"
    }

    fn refresh(&self, _sink: &ProgressSink, _cancel: &CancelToken) -> Result<()> {
        let txn = self.txn()?;
        txn.set_hints(&["interactive=true"])
            .map_err(|e| Error::PackageKit(e.to_string()))?;
        txn.refresh_cache(false)
            .map_err(|e| Error::PackageKit(e.to_string()))?;
        Ok(())
    }

    fn search(
        &self,
        q: &str,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> Result<Vec<PackageSummary>> {
        let txn = self.txn()?;
        let filter = (Filter::Newest as u64) | (Filter::Arch as u64);

        txn.resolve(filter, &[q])
            .map_err(|e| Error::PackageKit(e.to_string()))?;

        let (_details, packages) =
            collect(txn, cancel, &mut send_progress(sink, Stage::Searching))?;

        let mut results: Vec<PackageSummary> = packages
            .into_iter()
            .map(|p| {
                let (name, version) = parse_pkg_id(&p.package_id);
                PackageSummary {
                    id: PackageId {
                        name,
                        source: Source::Repo,
                        repo: None,
                    },
                    version,
                    description: p.summary,
                    installed: false,
                    popular: None,
                    last_updated: None,
                }
            })
            .collect();
        results.sort_by(|a, b| a.id.name.cmp(&b.id.name));
        results.dedup_by(|a, b| a.id.name == b.id.name);
        Ok(results)
    }

    fn details(
        &self,
        id: &PackageId,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> Result<PackageDetails> {
        let txn = self.txn()?;
        txn.resolve(Filter::Newest as u64 | Filter::Arch as u64, &[&id.name])
            .map_err(|e| Error::PackageKit(e.to_string()))?;
        let (_details, packages) =
            collect(txn, cancel, &mut send_progress(sink, Stage::Searching))?;

        let first_pkg = packages
            .first()
            .ok_or_else(|| Error::PackageKit(format!("{} not found", id.name)))?;
        let (_name, version) = parse_pkg_id(&first_pkg.package_id);

        Ok(PackageDetails {
            summary: PackageSummary {
                id: id.clone(),
                version,
                description: first_pkg.summary.clone(),
                installed: false,
                popular: None,
                last_updated: None,
            },
            description: None,
            depends: vec![],
            opt_depends: vec![],
            makedepends: vec![],
            conflicts: vec![],
            homepage: None,
            license: None,
            maintainer: None,
            developer: None,
            size_install: None,
            size_download: None,
        })
    }

    fn installed(&self, cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        let txn = self.txn()?;
        txn.get_packages(Filter::Installed as u64)
            .map_err(|e| Error::PackageKit(e.to_string()))?;
        let (_details, packages) = collect(txn, cancel, &mut |_| {})?;

        Ok(packages
            .into_iter()
            .map(|p| {
                let (name, version) = parse_pkg_id(&p.package_id);
                PackageSummary {
                    id: PackageId {
                        name,
                        source: Source::Repo,
                        repo: None,
                    },
                    version,
                    description: p.summary,
                    installed: true,
                    popular: None,
                    last_updated: None,
                }
            })
            .collect())
    }

    fn updates(&self, cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        let txn = self.txn()?;
        txn.get_updates(Filter::None as u64)
            .map_err(|e| Error::PackageKit(e.to_string()))?;
        let (_details, packages) = collect(txn, cancel, &mut |_| {})?;

        let mut results: Vec<PackageSummary> = packages
            .into_iter()
            .map(|p| {
                let (name, version) = parse_pkg_id(&p.package_id);
                PackageSummary {
                    id: PackageId {
                        name,
                        source: Source::Repo,
                        repo: None,
                    },
                    version,
                    description: p.summary,
                    installed: true,
                    popular: None,
                    last_updated: None,
                }
            })
            .collect();
        results.sort_by(|a, b| a.id.name.cmp(&b.id.name));
        results.dedup_by(|a, b| a.id.name == b.id.name);
        Ok(results)
    }

    fn operation(
        &self,
        op: &Operation,
        sink: &ProgressSink,
        cancel: &CancelToken,
        _progress: Box<dyn FnMut(f32) + Send + 'static>,
    ) -> Result<()> {
        let package_ids: Vec<&str> = op.package_ids.iter().map(|p| p.name.as_str()).collect();

        let txn = self.txn()?;

        let filter = match op.kind {
            OperationKind::Remove { .. } => Filter::Installed as u64,
            _ => (Filter::NotInstalled as u64) | (Filter::Newest as u64) | (Filter::Arch as u64),
        };
        txn.resolve(filter, &package_ids)
            .map_err(|e| Error::PackageKit(e.to_string()))?;
        let (_detail, resolved) = collect(txn, cancel, &mut send_progress(sink, Stage::Resolving))?;
        let resolved_ids: Vec<String> = resolved.into_iter().map(|p| p.package_id).collect();
        let refs: Vec<&str> = resolved_ids.iter().map(|s| s.as_str()).collect();

        if refs.is_empty() {
            return Err(Error::PackageKit("no packages resolved".into()));
        }

        let txn = self.txn()?;
        txn.set_hints(&["interactive=true"])
            .map_err(|e| Error::PackageKit(e.to_string()))?;

        let op_stage = match &op.kind {
            OperationKind::Install => {
                txn.install_packages(TxnFlag::OnlyTrusted as u64, &refs)
                    .map_err(|e| Error::PackageKit(e.to_string()))?;
                Stage::Installing
            }
            OperationKind::Remove { .. } => {
                txn.remove_packages(0, &refs, true, true)
                    .map_err(|e| Error::PackageKit(e.to_string()))?;
                Stage::Removing
            }
            OperationKind::Update => {
                txn.update_packages(TxnFlag::OnlyTrusted as u64, &refs)
                    .map_err(|e| Error::PackageKit(e.to_string()))?;
                Stage::Installing
            }
            OperationKind::Refresh => {
                txn.refresh_cache(false)
                    .map_err(|e| Error::PackageKit(e.to_string()))?;
                Stage::Refreshing
            }
        };

        let _ = collect(txn, cancel, &mut send_progress(sink, op_stage))?;

        Ok(())
    }

    fn install(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let txn = self.txn()?;
        txn.set_hints(&["interactive=true"])
            .map_err(|e| Error::PackageKit(e.to_string()))?;
        txn.install_packages(TxnFlag::OnlyTrusted as u64, &[&id.name])
            .map_err(|e| Error::PackageKit(e.to_string()))?;
        let _ = collect(txn, cancel, &mut send_progress(sink, Stage::Installing))?;
        Ok(())
    }

    fn remove(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let txn = self.txn()?;
        txn.set_hints(&["interactive=true"])
            .map_err(|e| Error::PackageKit(e.to_string()))?;
        txn.remove_packages(0, &[&id.name], true, true)
            .map_err(|e| Error::PackageKit(e.to_string()))?;
        let _ = collect(txn, cancel, &mut send_progress(sink, Stage::Removing))?;
        Ok(())
    }

    fn upgrade(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let txn = self.txn()?;
        txn.set_hints(&["interactive=true"])
            .map_err(|e| Error::PackageKit(e.to_string()))?;
        txn.update_packages(TxnFlag::OnlyTrusted as u64, &[&id.name])
            .map_err(|e| Error::PackageKit(e.to_string()))?;
        let _ = collect(txn, cancel, &mut send_progress(sink, Stage::Installing))?;
        Ok(())
    }

    fn upgrade_all(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let updates = self.updates(cancel)?;
        if updates.is_empty() {
            return Ok(());
        }

        let txn = self.txn()?;
        txn.set_hints(&["interactive=true"])
            .map_err(|e| Error::PackageKit(e.to_string()))?;
        let names: Vec<&str> = updates.iter().map(|u| u.id.name.as_str()).collect();
        txn.update_packages(TxnFlag::OnlyTrusted as u64, &names)
            .map_err(|e| Error::PackageKit(e.to_string()))?;
        let _ = collect(txn, cancel, &mut send_progress(sink, Stage::Installing))?;
        Ok(())
    }
}
