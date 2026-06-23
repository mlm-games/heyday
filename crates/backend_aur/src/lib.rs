use domain::*;
use serde::Deserialize;
use std::{
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
    results: Vec<T>,
}

#[derive(Deserialize)]
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
    fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| {
            let ext = p.extension().and_then(|e| e.to_str()) == Some("zst");
            let stem = p
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.starts_with(name));
            ext && stem
        })
}

impl PackageBackend for AurBackend {
    fn name(&self) -> &'static str {
        "aur"
    }

    fn refresh(&self, _sink: &ProgressSink, _cancel: &CancelToken) -> Result<()> {
        Ok(())
    }

    fn installed(&self, _cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        Ok(vec![])
    }

    fn updates(&self, _cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        Ok(vec![]) // Not preferable
    }

    fn operation(
        &self,
        _op: &Operation,
        _sink: &ProgressSink,
        _cancel: &CancelToken,
        _progress: Box<dyn FnMut(f32) + Send + 'static>,
    ) -> Result<()> {
        Err(Error::Aur(
            "direct operation not supported, use install/remove".into(),
        ))
    }

    fn search(
        &self,
        q: &str,
        sink: &ProgressSink,
        _cancel: &CancelToken,
    ) -> Result<Vec<PackageSummary>> {
        let q = q.trim();
        if q.len() < 2 {
            sink.send(Progress {
                job_id: 0,
                stage: Stage::Searching,
                percent: None,
                bytes: None,
                log: Some("AUR: query too short (<2), ignoring".into()),
                warning: true,
            })
            .ok();
            return Ok(vec![]);
        }

        sink.send(Progress {
            job_id: 0,
            stage: Stage::Searching,
            percent: None,
            bytes: None,
            log: Some(format!("AUR search: {q}")),
            warning: false,
        })
        .ok();

        // Be explicit about name+description search to match user expectations
        // RPC v5 docs note 2+ chars and rate limiting; keep the guard above.
        let url = format!(
            "https://aur.archlinux.org/rpc/?v=5&type=search&by=name-desc&arg={}",
            urlencoding::encode(q)
        );
        let mut resp = http()
            .get(&url)
            .call()
            .map_err(|e| Error::Network(e.to_string()))?;
        let resp: AurResponse<AurPkg> = resp
            .body_mut()
            .read_json()
            .map_err(|e| Error::Network(e.to_string()))?;

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
        _sink: &ProgressSink,
        _cancel: &CancelToken,
    ) -> Result<PackageDetails> {
        let url = format!(
            "https://aur.archlinux.org/rpc/?v=5&type=info&arg[]={}",
            urlencoding::encode(&id.name)
        );
        let mut resp = ureq::get(&url)
            .call()
            .map_err(|e| Error::Network(e.to_string()))?;
        let resp: AurResponse<AurPkg> = resp
            .body_mut()
            .read_json()
            .map_err(|e| Error::Network(e.to_string()))?;
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
            depends: p.depends.unwrap_or_default(),
            opt_depends: opt_names,
            homepage: p.url,
            maintainer: p.maintainer,
            size_install: None,
            size_download: None,
        })
    }

    fn install(&self, id: &PackageId, sink: &ProgressSink, _cancel: &CancelToken) -> Result<()> {
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

        let work = tempfile::tempdir().map_err(|e| Error::Internal(e.to_string()))?;
        let dir = work.path().join(&id.name);

        // Shallow clone to reduce bandwidth
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

        send_log(Stage::Building, "generating .SRCINFO", false);

        // Generate .SRCINFO (no shell redirection)
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

        send_log(
            Stage::Building,
            &format!("running makepkg for {}", id.name),
            false,
        );

        // Build package (no -i here)
        let status = Command::new("makepkg")
            .args(["-s", "--noconfirm"])
            .current_dir(&dir)
            .status()
            .map_err(|e| Error::Internal(e.to_string()))?;
        if !status.success() {
            return Err(Error::Aur("makepkg failed".into()));
        }

        send_log(
            Stage::Installing,
            &format!("installing {} via pacman -U", id.name),
            false,
        );

        // Install artifact via pacman -U
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

    fn remove(&self, id: &PackageId, _sink: &ProgressSink, _cancel: &CancelToken) -> Result<()> {
        let code = Command::new("pkexec")
            .args(["pacman", "-Rns", "--noconfirm", &id.name])
            .status()
            .map_err(|e| Error::Priv(e.to_string()))?;
        if code.success() {
            Ok(())
        } else {
            Err(Error::Priv("remove failed".into()))
        }
    }
    fn upgrade(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        // For AUR, “upgrade” is just “rebuild & install latest”.
        self.install(id, sink, cancel)
    }

    fn upgrade_all(&self, _sink: &ProgressSink, _cancel: &CancelToken) -> Result<()> {
        // Minimal first step: do nothing. We can iterate available AUR upgrades later.
        Ok(())
    }
}
