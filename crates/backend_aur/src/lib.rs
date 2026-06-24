use domain::*;
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::PathBuf,
    process::Command,
    sync::LazyLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use ureq::config::Config;

#[derive(Deserialize)]
struct AurResponse<T> {
    #[serde(default)]
    results: Vec<T>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Default, Deserialize)]
pub struct AurPkg {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Version")]
    version: String,
    #[serde(rename = "Description")]
    description: Option<String>,
    #[serde(rename = "NumVotes")]
    votes: Option<u32>,
    #[serde(rename = "Maintainer")]
    maintainer: Option<String>,
    #[serde(rename = "LastModified")]
    last_modified: Option<u64>,
    #[serde(rename = "URL")]
    url: Option<String>,
    #[serde(rename = "License", default)]
    license: Option<Vec<String>>,
    #[serde(rename = "Depends")]
    depends: Option<Vec<String>>,
    #[serde(rename = "OptDepends")]
    optdepends: Option<Vec<String>>,
}

pub struct AurBackend;
impl AurBackend {
    pub fn new() -> Self {
        Self
    }
}

static HTTP: LazyLock<ureq::Agent> = LazyLock::new(|| {
    ureq::Agent::new_with_config(
        Config::builder()
            .timeout_global(Some(Duration::from_secs(7)))
            .build(),
    )
});

fn http() -> &'static ureq::Agent {
    &HTTP
}

fn ts(opt: Option<u64>) -> Option<SystemTime> {
    opt.map(|t| UNIX_EPOCH + std::time::Duration::from_secs(t))
}

fn parse_srcinfo_deps(srcinfo: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in srcinfo.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("depends = ") {
            out.push(strip_ver(v));
        } else if let Some(v) = line.strip_prefix("makedepends = ") {
            out.push(strip_ver(v));
        }
    }
    out.sort();
    out.dedup();
    out
}

fn strip_ver(s: &str) -> String {
    s.split(|c| c == '<' || c == '>' || c == '=')
        .next()
        .unwrap_or(s)
        .trim()
        .to_string()
}

fn find_built_pkg(name: &str, dir: &PathBuf) -> Option<PathBuf> {
    let prefix = format!("{name}-");
    fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| {
            let ext = p.extension().and_then(|e| e.to_str()) == Some("zst");
            let stem = p
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.starts_with(&prefix));
            ext && stem
        })
}

fn foreign_packages() -> Vec<(String, String)> {
    let ver_out = match Command::new("pacman").args(["-Qm"]).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return vec![],
    };
    let mut pkgs = Vec::new();
    for line in ver_out.lines() {
        let line = line.trim();
        if let Some((name, ver)) = line.split_once(' ') {
            pkgs.push((name.to_string(), ver.to_string()));
        }
    }
    pkgs
}

fn aur_info(names: &[String]) -> Vec<AurPkg> {
    if names.is_empty() {
        return vec![];
    }
    let mut url = format!("https://aur.archlinux.org/rpc/?v=5&type=info");
    for n in names {
        use std::fmt::Write;
        write!(url, "&arg[]={}", urlencoding::encode(n)).ok();
    }
    let mut resp = match http().get(&url).call() {
        Ok(r) => r,
        Err(e) => {
            log::warn!("AUR info RPC failed: {e}");
            return vec![];
        }
    };
    match resp.body_mut().read_json::<AurResponse<AurPkg>>() {
        Ok(r) => r.results,
        Err(e) => {
            log::warn!("AUR info RPC parse failed: {e}");
            vec![]
        }
    }
}

fn vercmp(v1: &str, v2: &str) -> std::cmp::Ordering {
    // vercmp binary prints -1/0/1 to stdout and always exits 0
    let out = match Command::new("vercmp").args([v1, v2]).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => return std::cmp::Ordering::Equal,
    };
    match out.as_str() {
        "-1" => std::cmp::Ordering::Less,
        "1" => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    }
}

impl PackageBackend for AurBackend {
    fn name(&self) -> &'static str {
        "aur"
    }

    fn refresh(&self, _sink: &ProgressSink, _cancel: &CancelToken) -> Result<()> {
        Ok(())
    }

    fn installed(&self, _cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        let foreign = foreign_packages();
        if foreign.is_empty() {
            return Ok(vec![]);
        }
        let names: Vec<String> = foreign.iter().map(|(n, _)| n.clone()).collect();
        let info = aur_info(&names);
        let mut by_name: HashMap<&str, &AurPkg> = HashMap::new();
        for p in &info {
            by_name.insert(p.name.as_str(), p);
        }
        Ok(foreign
            .into_iter()
            .map(|(name, version)| {
                let p = by_name.get(name.as_str());
                PackageSummary {
                    id: PackageId {
                        name: name.clone(),
                        source: Source::Aur,
                        repo: None,
                    },
                    version: version.clone(),
                    description: p.and_then(|p| p.description.clone()).unwrap_or_default(),
                    installed: true,
                    popular: p.and_then(|p| p.votes),
                    last_updated: p.and_then(|p| ts(p.last_modified)),
                }
            })
            .collect())
    }

    fn updates(&self, _cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        let foreign = foreign_packages();
        if foreign.is_empty() {
            return Ok(vec![]);
        }
        let names: Vec<String> = foreign.iter().map(|(n, _)| n.clone()).collect();
        let info = aur_info(&names);
        let mut results = Vec::new();
        for p in &info {
            if let Some((_, installed_ver)) = foreign.iter().find(|(n, _)| n == &p.name) {
                if vercmp(installed_ver, &p.version) == std::cmp::Ordering::Less {
                    results.push(PackageSummary {
                        id: PackageId {
                            name: p.name.clone(),
                            source: Source::Aur,
                            repo: None,
                        },
                        version: p.version.clone(),
                        description: p.description.clone().unwrap_or_default(),
                        installed: true,
                        popular: p.votes,
                        last_updated: ts(p.last_modified),
                    });
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
        for id in &op.package_ids {
            match op.kind {
                OperationKind::Install => self.install(id, sink, cancel)?,
                OperationKind::Remove { .. } => self.remove(id, sink, cancel)?,
                OperationKind::Update => self.upgrade(id, sink, cancel)?,
                OperationKind::Refresh => self.refresh(sink, cancel)?,
            }
        }
        Ok(())
    }

    fn search(
        &self,
        q: &str,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> Result<Vec<PackageSummary>> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let q = q.trim();
        if q.len() < 2 {
            let _ = sink.send(Progress {
                job_id: 0,
                stage: Stage::Searching,
                percent: None,
                bytes: None,
                log: Some("AUR: query too short (<2), ignoring".into()),
                warning: true,
            });
            return Ok(vec![]);
        }

        let _ = sink.send(Progress {
            job_id: 0,
            stage: Stage::Searching,
            percent: None,
            bytes: None,
            log: Some(format!("AUR search: {q}")),
            warning: false,
        });

        let url = format!(
            "https://aur.archlinux.org/rpc/?v=5&type=search&by=name-desc&arg={}",
            urlencoding::encode(q)
        );
        let mut resp = match http().get(&url).call() {
            Ok(r) => r,
            Err(e) => {
                let _ = sink.send(Progress {
                    job_id: 0,
                    stage: Stage::Searching,
                    percent: None,
                    bytes: None,
                    log: Some(format!("AUR search failed: {e}")),
                    warning: true,
                });
                return Ok(vec![]);
            }
        };
        let resp: AurResponse<AurPkg> = match resp.body_mut().read_json() {
            Ok(r) => r,
            Err(e) => {
                let _ = sink.send(Progress {
                    job_id: 0,
                    stage: Stage::Searching,
                    percent: None,
                    bytes: None,
                    log: Some(format!("AUR search parse failed: {e}")),
                    warning: true,
                });
                return Ok(vec![]);
            }
        };

        if let Some(err) = resp.error {
            let _ = sink.send(Progress {
                job_id: 0,
                stage: Stage::Searching,
                percent: None,
                bytes: None,
                log: Some(format!("AUR error: {err}")),
                warning: true,
            });
            return Ok(vec![]);
        }

        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        let installed = installed_package_names();

        Ok(resp
            .results
            .into_iter()
            .map(|p| PackageSummary {
                id: PackageId {
                    name: p.name.clone(),
                    source: Source::Aur,
                    repo: Default::default(),
                },
                version: p.version,
                description: p.description.unwrap_or_default(),
                installed: installed.contains(&p.name),
                popular: p.votes,
                last_updated: ts(p.last_modified),
            })
            .collect())
    }

    fn details(
        &self,
        id: &PackageId,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> Result<PackageDetails> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let url = format!(
            "https://aur.archlinux.org/rpc/?v=5&type=info&arg[]={}",
            urlencoding::encode(&id.name)
        );
        let mut resp = match http().get(&url).call() {
            Ok(r) => r,
            Err(e) => {
                let _ = sink.send(Progress {
                    job_id: 0,
                    stage: Stage::Searching,
                    percent: None,
                    bytes: None,
                    log: Some(format!("AUR info failed: {e}")),
                    warning: true,
                });
                return Err(Error::Network(e.to_string()));
            }
        };
        let resp: AurResponse<AurPkg> = match resp.body_mut().read_json() {
            Ok(r) => r,
            Err(e) => {
                let _ = sink.send(Progress {
                    job_id: 0,
                    stage: Stage::Searching,
                    percent: None,
                    bytes: None,
                    log: Some(format!("AUR info parse failed: {e}")),
                    warning: true,
                });
                return Err(Error::Network(e.to_string()));
            }
        };

        if let Some(err) = resp.error {
            return Err(Error::Aur(err));
        }

        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        let p = resp
            .results
            .into_iter()
            .next()
            .ok_or_else(|| Error::Aur("not found".into()))?;

        let installed = installed_package_names();

        let summary = PackageSummary {
            id: PackageId {
                name: p.name.clone(),
                source: Source::Aur,
                repo: None,
            },
            version: p.version,
            description: p.description.unwrap_or_default(),
            installed: installed.contains(&p.name),
            popular: p.votes,
            last_updated: ts(p.last_modified),
        };
        let opt_names = p
            .optdepends
            .unwrap_or_default()
            .into_iter()
            .filter_map(|s| s.split(':').next().map(|n| n.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        Ok(PackageDetails {
            summary,
            description: None,
            depends: p.depends.unwrap_or_default(),
            opt_depends: opt_names,
            homepage: p.url,
            license: p.license.as_ref().map(|v| v.join(", ")),
            maintainer: p.maintainer,
            developer: None,
            size_install: None,
            size_download: None,
        })
    }

    fn install(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let send_log = |stage: Stage, msg: &str, warning: bool| {
            let _ = sink.send(Progress {
                job_id: 0,
                stage,
                percent: None,
                bytes: None,
                log: Some(msg.into()),
                warning,
            });
        };

        send_log(Stage::Building, &format!("cloning {}.git", id.name), false);

        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        let work = tempfile::tempdir().map_err(|e| Error::Internal(e.to_string()))?;
        let dir = work.path().join(&id.name);

        let dir_str = dir
            .to_str()
            .ok_or_else(|| Error::Internal("non-UTF-8 temp dir path".into()))?
            .to_string();
        let status = Command::new("git")
            .args([
                "clone",
                "--depth=1",
                &format!("https://aur.archlinux.org/{}.git", id.name),
                &dir_str,
            ])
            .status()
            .map_err(|e| Error::Internal(e.to_string()))?;
        if !status.success() {
            return Err(Error::Aur("git clone failed".into()));
        }

        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        send_log(Stage::Building, "generating .SRCINFO", false);

        let out = Command::new("makepkg")
            .arg("--printsrcinfo")
            .current_dir(&dir)
            .output()
            .map_err(|e| Error::Internal(e.to_string()))?;
        if !out.status.success() {
            return Err(Error::Aur("printsrcinfo failed".into()));
        }
        let mut f =
            fs::File::create(dir.join(".SRCINFO")).map_err(|e| Error::Internal(e.to_string()))?;
        f.write_all(&out.stdout)
            .map_err(|e| Error::Internal(e.to_string()))?;

        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        // Preinstall repo deps best-effort
        let srcinfo = String::from_utf8_lossy(&out.stdout);
        let deps = parse_srcinfo_deps(&srcinfo);
        if !deps.is_empty() {
            send_log(
                Stage::Resolving,
                &format!("preinstalling deps: {}", deps.join(", ")),
                false,
            );
            let dep_status = Command::new("pkexec")
                .args(["pacman", "-S", "--noconfirm", "--needed"])
                .args(deps.iter().map(|s| s.as_str()))
                .status();
            match dep_status {
                Ok(s) if !s.success() => {
                    send_log(
                        Stage::Resolving,
                        &format!(
                            "preinstall deps exited with code {}",
                            s.code().unwrap_or(-1)
                        ),
                        true,
                    );
                }
                Err(e) => {
                    send_log(
                        Stage::Resolving,
                        &format!("preinstall deps failed: {e}"),
                        true,
                    );
                }
                _ => {}
            }
        }

        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        send_log(
            Stage::Building,
            &format!("running makepkg for {}", id.name),
            false,
        );

        let status = Command::new("makepkg")
            .args(["-s", "--noconfirm"])
            .current_dir(&dir)
            .status()
            .map_err(|e| Error::Internal(e.to_string()))?;
        if !status.success() {
            return Err(Error::Aur("makepkg failed".into()));
        }

        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        send_log(
            Stage::Installing,
            &format!("installing {} via pacman -U", id.name),
            false,
        );

        let pkg = find_built_pkg(&id.name, &dir)
            .ok_or_else(|| Error::Aur("no built package found".into()))?;
        let pkg_str = pkg
            .to_str()
            .ok_or_else(|| Error::Internal("non-UTF-8 built package path".into()))?
            .to_string();
        let out = Command::new("pkexec")
            .args(["pacman", "-U", "--noconfirm", &pkg_str])
            .output()
            .map_err(|e| Error::Priv(e.to_string()))?;
        if out.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let detail = if stderr.trim().is_empty() {
                "pacman -U failed: see log".into()
            } else {
                format!("pacman -U failed: {}", stderr.trim())
            };
            Err(Error::Priv(detail))
        }
    }

    fn remove(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let _ = sink.send(Progress {
            job_id: 0,
            stage: Stage::Removing,
            percent: None,
            bytes: None,
            log: Some(format!("removing {}", id.name)),
            warning: false,
        });
        let out = Command::new("pkexec")
            .args(["pacman", "-Rns", "--noconfirm", &id.name])
            .output()
            .map_err(|e| Error::Priv(e.to_string()))?;
        if out.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&out.stderr);
            Err(Error::Priv(format!("remove failed: {}", stderr.trim())))
        }
    }

    fn upgrade(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        self.install(id, sink, cancel)
    }

    fn upgrade_all(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let updates = self.updates(cancel)?;
        for pkg in &updates {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            self.install(&pkg.id, sink, cancel)?;
        }
        Ok(())
    }
}
