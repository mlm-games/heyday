use domain::*;
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
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
    #[serde(rename = "MakeDepends")]
    makedepends: Option<Vec<String>>,
    #[serde(rename = "OptDepends")]
    optdepends: Option<Vec<String>>,
    #[serde(rename = "Conflicts")]
    conflicts: Option<Vec<String>>,
    #[serde(rename = "PackageBase")]
    package_base: Option<String>,
}

pub struct AurBackend;
impl AurBackend {
    pub fn new() -> Self {
        Self
    }

    fn prepare_package(
        &self,
        id: &PackageId,
        version: &str,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> Result<PreparedPkg> {
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

        let pkgbase = pkgbase_for(&id.name);

        send_log(Stage::Building, &format!("cloning {}.git", pkgbase), false);

        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        let work = tempfile::tempdir().map_err(|e| Error::Internal(e.to_string()))?;
        let dir = work.path().join(&pkgbase);

        let dir_str = dir
            .to_str()
            .ok_or_else(|| Error::Internal("non-UTF-8 temp dir path".into()))?
            .to_string();
        let status = Command::new("git")
            .args([
                "clone",
                "--depth=1",
                &format!("https://aur.archlinux.org/{pkgbase}.git"),
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

        let srcinfo = String::from_utf8_lossy(&out.stdout);
        let deps = parse_srcinfo_deps(&srcinfo);
        let conflicts = parse_conflicts(&srcinfo);

        Ok(PreparedPkg {
            work,
            dir,
            id: id.clone(),
            version: version.to_string(),
            deps,
            conflicts,
        })
    }

    fn build_prepared(
        &self,
        pkg: &PreparedPkg,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> Result<PathBuf> {
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

        let missing: Vec<&String> = pkg
            .deps
            .iter()
            .filter(|d| {
                Command::new("pacman")
                    .args(["-T", d])
                    .output()
                    .ok()
                    .is_none_or(|o| !o.status.success())
            })
            .collect();
        if !missing.is_empty() {
            send_log(
                Stage::Resolving,
                &format!(
                    "installing deps: {}",
                    missing
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                false,
            );
            let dep_status = Command::new("pkexec")
                .args(["pacman", "-S", "--noconfirm", "--needed"])
                .args(missing.iter().map(|s| s.as_str()))
                .status();

            match dep_status {
                Ok(s) if !s.success() => {
                    send_log(
                        Stage::Resolving,
                        &format!("dep install exited with code {}", s.code().unwrap_or(-1)),
                        true,
                    );
                }
                Err(e) => {
                    send_log(Stage::Resolving, &format!("dep install failed: {e}"), true);
                }
                _ => {}
            }
        }

        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        send_log(
            Stage::Building,
            &format!("running makepkg for {}", pkg.id.name),
            false,
        );

        let status = Command::new("makepkg")
            .args(["--noconfirm"])
            .current_dir(&pkg.dir)
            .status()
            .map_err(|e| Error::Internal(e.to_string()))?;
        if !status.success() {
            return Err(Error::Aur("makepkg failed".into()));
        }

        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        find_built_pkg(&pkg.id.name, &pkg.dir)
            .ok_or_else(|| Error::Aur("no built package found".into()))
    }

    fn build_package(
        &self,
        id: &PackageId,
        sink: &ProgressSink,
        cancel: &CancelToken,
    ) -> Result<(tempfile::TempDir, PathBuf)> {
        let pkg = self.prepare_package(id, "", sink, cancel)?;
        let pkg_path = self.build_prepared(&pkg, sink, cancel)?;
        Ok((pkg.work, pkg_path))
    }

    fn pkexec_install(&self, pkgs: &[PathBuf], sink: &ProgressSink) -> Result<()> {
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

        let names: Vec<&str> = pkgs
            .iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()))
            .collect();

        send_log(
            Stage::Installing,
            &format!("installing {} package(s): {}", pkgs.len(), names.join(", ")),
            false,
        );

        let pkg_strs: Vec<String> = pkgs
            .iter()
            .map(|p| {
                p.to_str()
                    .map(|s| s.to_string())
                    .ok_or_else(|| Error::Internal("non-UTF-8 package path".into()))
            })
            .collect::<Result<Vec<_>>>()?;

        let mut cmd = Command::new("pkexec");
        cmd.arg("pacman");
        cmd.arg("-U");
        cmd.arg("--noconfirm");
        for s in &pkg_strs {
            cmd.arg(s);
        }
        let out = cmd.output().map_err(|e| Error::Priv(e.to_string()))?;

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

fn parse_conflicts(srcinfo: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in srcinfo.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("conflicts = ") {
            out.push(strip_ver(v));
        }
    }
    out
}

fn needs_build(name: &str, version: &str) -> bool {
    if version.is_empty() {
        return true;
    }
    Command::new("pacman")
        .args(["-Q", name])
        .output()
        .ok()
        .is_none_or(|o| {
            if !o.status.success() {
                return true;
            }
            let out = String::from_utf8_lossy(&o.stdout);
            !out.trim().contains(version)
        })
}

struct PreparedPkg {
    work: tempfile::TempDir,
    dir: PathBuf,
    id: PackageId,
    version: String,
    deps: Vec<String>,
    conflicts: Vec<String>,
}

fn sort_build_order(pkgs: &[PreparedPkg]) -> Result<Vec<usize>> {
    let n = pkgs.len();
    let mut in_degree = vec![0; n];
    let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();

    let name_to_idx: HashMap<&str, usize> = pkgs
        .iter()
        .enumerate()
        .map(|(i, p)| (p.id.name.as_str(), i))
        .collect();

    for (i, pkg) in pkgs.iter().enumerate() {
        for dep in &pkg.deps {
            if let Some(&j) = name_to_idx.get(dep.as_str()) {
                adj.entry(j).or_default().push(i);
                in_degree[i] += 1;
            }
        }
    }

    let mut queue: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
    let mut order = Vec::new();

    while let Some(i) = queue.pop() {
        order.push(i);
        if let Some(neighbors) = adj.get(&i) {
            for &j in neighbors {
                in_degree[j] -= 1;
                if in_degree[j] == 0 {
                    queue.push(j);
                }
            }
        }
    }

    if order.len() != n {
        return Err(Error::Aur(
            "circular dependency detected in AUR packages".into(),
        ));
    }

    Ok(order)
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
    let mut results = Vec::new();
    for chunk in names.chunks(100) {
        let mut form: Vec<(&str, &str)> = vec![("v", "5"), ("type", "info")];
        for n in chunk {
            form.push(("arg[]", n));
        }
        let mut resp = match http()
            .post("https://aur.archlinux.org/rpc/")
            .send_form(form)
        {
            Ok(r) => r,
            Err(e) => {
                log::warn!("AUR info RPC failed: {e}");
                return results;
            }
        };
        match resp.body_mut().read_json::<AurResponse<AurPkg>>() {
            Ok(r) => results.extend(r.results),
            Err(e) => {
                log::warn!("AUR info RPC parse failed: {e}");
                return results;
            }
        }
    }
    results
}

fn vercmp(v1: &str, v2: &str) -> Ordering {
    alpm::vercmp(v1, v2)
}

fn pkgbase_for(name: &str) -> String {
    let info = aur_info(&[name.to_string()]);
    info.first()
        .and_then(|p| p.package_base.clone())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| name.to_string())
}

// ── VCS update detection ──────────────────────────────────────────

const VCS_SUFFIXES: &[&str] = &["-git", "-svn", "-hg", "-bzr", "-darcs", "-cvs"];

fn is_vcs(name: &str) -> bool {
    VCS_SUFFIXES.iter().any(|s| name.ends_with(s))
}

#[derive(Serialize, Deserialize)]
struct VcsEntry {
    url: String,
    branch: String,
    commit: String,
}

#[derive(Serialize, Deserialize, Default)]
struct VcsStore {
    packages: HashMap<String, Vec<VcsEntry>>,
}

fn vcs_store_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".cache/soredowe/aur-vcs.json")
}

fn load_vcs_store() -> VcsStore {
    let path = vcs_store_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_vcs_store(store: &VcsStore) {
    let path = vcs_store_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string_pretty(store) {
        let _ = fs::write(&path, &s);
    }
}

fn fetch_pkgbuild(pkgbase: &str) -> Option<String> {
    let url = format!("https://aur.archlinux.org/cgit/aur.git/plain/PKGBUILD?h={pkgbase}");
    let mut resp = http().get(&url).call().ok()?;
    resp.body_mut().read_to_string().ok()
}

fn parse_git_sources(pkgbuild: &str) -> Vec<(String, String)> {
    let mut sources = Vec::new();
    let mut in_source = false;
    let mut buf = String::new();

    for line in pkgbuild.lines() {
        let line = line.trim();

        if !in_source {
            if let Some(rest) = line.strip_prefix("source=") {
                let rest = rest.trim();
                if rest.is_empty() {
                    continue;
                }
                if let Some(inner) = rest.strip_prefix('(') {
                    buf = inner.to_string();
                    in_source = true;
                    if let Some(stripped) = inner.strip_suffix(')') {
                        buf = stripped.to_string();
                        in_source = false;
                    }
                }
            }
        } else if let Some(stripped) = line.strip_suffix(')') {
            buf.push_str(stripped);
            in_source = false;
        } else {
            buf.push_str(line);
        }
    }

    for token in buf.split('"') {
        let token = token.trim();
        if token.is_empty() || !(token.contains("://") || token.starts_with("git@")) {
            continue;
        }
        let url = if let Some(idx) = token.find("::") {
            &token[idx + 2..]
        } else {
            token
        };
        let (url, branch) = if let Some(idx) = url.find('#') {
            (&url[..idx], Some(&url[idx + 1..]))
        } else {
            (url, None)
        };
        if url.contains("://") || url.starts_with("git@") {
            sources.push((url.to_string(), branch.unwrap_or("HEAD").to_string()));
        }
    }
    sources
}

fn git_ls_remote(url: &str, branch: &str) -> Option<String> {
    let refspec = if branch == "HEAD" {
        "HEAD".to_string()
    } else {
        format!("refs/heads/{branch}")
    };
    let out = Command::new("git")
        .args(["ls-remote", url, &refspec])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .lines()
        .next()
        .and_then(|line| line.split('\t').next().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
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
        let mut checked = std::collections::HashSet::new();

        for p in &info {
            checked.insert(p.name.clone());
            if let Some((_, installed_ver)) = foreign.iter().find(|(n, _)| n == &p.name)
                && vercmp(installed_ver, &p.version) == Ordering::Less
            {
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

        let vcs_pkgs: Vec<_> = foreign
            .iter()
            .filter(|(n, _)| is_vcs(n) && !checked.contains(n.as_str()))
            .collect();

        if !vcs_pkgs.is_empty() {
            let mut store = load_vcs_store();
            let mut dirty = false;

            for (name, _installed_ver) in &vcs_pkgs {
                let pkgbase = pkgbase_for(name);
                let Some(pkgbuild) = fetch_pkgbuild(&pkgbase) else {
                    continue;
                };
                let sources = parse_git_sources(&pkgbuild);
                if sources.is_empty() {
                    continue;
                }
                let mut needs_update = false;
                let mut new_entries = Vec::new();
                for (url, branch) in &sources {
                    if let Some(sha) = git_ls_remote(url, branch) {
                        let stored = store
                            .packages
                            .get(name.as_str())
                            .and_then(|entries| entries.iter().find(|e| e.url == *url));
                        if let Some(e) = stored
                            && e.commit != sha
                        {
                            needs_update = true;
                        }
                        new_entries.push(VcsEntry {
                            url: url.clone(),
                            branch: branch.clone(),
                            commit: sha,
                        });
                    }
                }
                if needs_update {
                    results.push(PackageSummary {
                        id: PackageId {
                            name: name.clone(),
                            source: Source::Aur,
                            repo: None,
                        },
                        version: "latest-commit".into(),
                        description: String::new(),
                        installed: true,
                        popular: None,
                        last_updated: None,
                    });
                }
                store.packages.insert(name.clone(), new_entries);
                dirty = true;
            }

            if dirty {
                save_vcs_store(&store);
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
            makedepends: p.makedepends.unwrap_or_default(),
            conflicts: p.conflicts.unwrap_or_default(),
            homepage: p.url,
            license: p.license.as_ref().map(|v| v.join(", ")),
            maintainer: p.maintainer,
            developer: None,
            size_install: None,
            size_download: None,
        })
    }
    fn install(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let (_work, pkg) = self.build_package(id, sink, cancel)?;
        self.pkexec_install(&[pkg], sink)
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
        if updates.is_empty() {
            return Ok(());
        }

        // 1: prepare all (clone + printsrcinfo + parse deps/conflicts)
        let mut prepared = Vec::new();
        for pkg in &updates {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            prepared.push(self.prepare_package(&pkg.id, &pkg.version, sink, cancel)?);
        }

        // 2: check for conflicts
        for p in &prepared {
            for conflict in &p.conflicts {
                if p.id.name == *conflict {
                    continue;
                }
                if prepared.iter().any(|q| q.id.name == *conflict) {
                    let _ = sink.send(Progress {
                        job_id: 0,
                        stage: Stage::Resolving,
                        percent: None,
                        bytes: None,
                        log: Some(format!(
                            "conflict: {} and {} in same batch",
                            p.id.name, conflict
                        )),
                        warning: true,
                    });
                }
                if Command::new("pacman")
                    .args(["-Q", conflict])
                    .output()
                    .ok()
                    .is_some_and(|o| o.status.success())
                {
                    let _ = sink.send(Progress {
                        job_id: 0,
                        stage: Stage::Resolving,
                        percent: None,
                        bytes: None,
                        log: Some(format!(
                            "conflict: {} conflicts with installed {}",
                            p.id.name, conflict
                        )),
                        warning: true,
                    });
                }
            }
        }

        // 3: topological sort by dependencies
        let order = sort_build_order(&prepared)?;

        // 4: build in dependency order
        let mut built = Vec::new();
        for &idx in &order {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let p = &prepared[idx];
            if needs_build(&p.id.name, &p.version) {
                built.push(self.build_prepared(p, sink, cancel)?);
            }
        }

        // 5: single pkexec install for all
        if built.is_empty() {
            Ok(())
        } else {
            self.pkexec_install(&built, sink)
        }
    }
}
