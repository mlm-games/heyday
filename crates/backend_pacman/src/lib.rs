use domain::*;
use parking_lot::Mutex;
use regex::Regex;
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::{Arc, LazyLock},
};

pub struct PacmanCli;
impl Default for PacmanCli {
    fn default() -> Self {
        Self::new()
    }
}
impl PacmanCli {
    pub fn new() -> Self {
        Self
    }

    fn parse_upgrades(out: &str) -> Vec<PackageSummary> {
        // Lines look like: "pkgname oldver -> newver"
        static RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^(?P<name>\S+)\s+\S+\s+->\s+(?P<new>\S+)").unwrap());
        out.lines()
            .filter_map(|l| {
                RE.captures(l).map(|c| PackageSummary {
                    id: PackageId {
                        name: c["name"].to_string(),
                        source: Source::Repo,
                    },
                    version: c["new"].to_string(),
                    description: String::new(),
                    installed: true,
                    popular: None,
                    last_updated: None,
                })
            })
            .collect()
    }

    fn search_fallback_names(&self, q: &str, sink: &ProgressSink) -> Result<Vec<PackageSummary>> {
        let installed = installed_package_names();
        let out = match std::process::Command::new("pacman")
            .args(["-Ssq", q])
            .output()
        {
            Ok(o) => o,
            Err(e) => {
                sink.send(Progress {
                    job_id: 0,
                    stage: Stage::Searching,
                    percent: None,
                    bytes: None,
                    log: Some(format!("repo: fallback -Ssq spawn failed: {e}")),
                    warning: true,
                })
                .ok();
                return Ok(vec![]);
            }
        };

        if !out.status.success() {
            sink.send(Progress {
                job_id: 0,
                stage: Stage::Searching,
                percent: None,
                bytes: None,
                log: Some(format!(
                    "repo: fallback -Ssq failed (exit {}), returning no repo items",
                    out.status.code().unwrap_or(-1)
                )),
                warning: true,
            })
            .ok();
            return Ok(vec![]);
        }

        let names = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .take(500) // avoid huge UI floods
            .map(|name| PackageSummary {
                id: PackageId {
                    name: name.to_string(),
                    source: Source::Repo,
                },
                version: String::new(),
                description: String::new(),
                installed: installed.contains(name),
                popular: None,
                last_updated: None,
            })
            .collect::<Vec<_>>();

        if names.is_empty() {
            sink.send(Progress {
                job_id: 0,
                stage: Stage::Searching,
                percent: None,
                bytes: None,
                log: Some("repo: fallback -Ssq returned 0 matches".into()),
                warning: false,
            })
            .ok();
        } else {
            sink.send(Progress {
                job_id: 0,
                stage: Stage::Searching,
                percent: None,
                bytes: None,
                log: Some(format!("repo: fallback -Ssq yielded {} names", names.len())),
                warning: false,
            })
            .ok();
        }

        Ok(names)
    }
}

/// parsing for -Ss
fn parse_pacman_search(out: &str) -> Vec<PackageSummary> {
    static RE_HEAD: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?P<repo>\S+)/(?P<name>\S+)\s+(?P<ver>\S+)(?:\s+\[installed.*\])?\s*$")
            .unwrap()
    });
    static RE_INST: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\[installed").unwrap());
    let mut res = Vec::new();
    let mut last: Option<PackageSummary> = None;
    for line in out.lines() {
        if let Some(c) = RE_HEAD.captures(line) {
            let name = c["name"].to_string();
            let ver = c["ver"].to_string();
            let installed = RE_INST.is_match(line);
            last = Some(PackageSummary {
                id: PackageId {
                    name,
                    source: Source::Repo,
                },
                version: ver,
                description: String::new(),
                installed,
                popular: None,
                last_updated: None,
            });
        } else if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(mut s) = last.take() {
                s.description = line.trim().to_string();
                res.push(s);
            }
        }
    }
    if let Some(s) = last.take() {
        res.push(s);
    }
    res
}

/// parsing for -Si
fn parse_pacman_details(out: &str, mut summary: PackageSummary) -> PackageDetails {
    fn strip_ver_token(tok: &str) -> String {
        tok.split(|c| c == '<' || c == '>' || c == '=')
            .next()
            .unwrap_or("")
            .trim()
            .to_string()
    }
    fn parse_pkg_list(s: &str) -> Vec<String> {
        s.split_whitespace()
            .filter_map(|t| {
                let n = strip_ver_token(t);
                if n.is_empty() || n == "None" {
                    None
                } else {
                    Some(n)
                }
            })
            .collect()
    }
    let mut depends = Vec::new();
    let mut opt_depends = Vec::new();
    let mut homepage = None;
    let mut size_install = None;
    let mut size_download = None;
    let mut maintainer = None;
    let mut in_dep = false;
    let mut in_opt = false;

    for raw in out.lines() {
        let line = raw.trim_end();

        if let Some(v) = line.strip_prefix("Depends On      :") {
            in_dep = true;
            in_opt = false;
            let v = v.trim();
            if v != "None" {
                depends.extend(parse_pkg_list(v));
            }
            continue;
        } else if let Some(v) = line.strip_prefix("Optional Deps   :") {
            in_dep = false;
            in_opt = true;
            let v = v.trim();
            if !v.is_empty() {
                if let Some(name) = v.split(':').next() {
                    let n = name.trim();
                    if !n.is_empty() {
                        opt_depends.push(n.to_string());
                    }
                }
            }
            continue;
        } else if let Some(v) = line.strip_prefix("URL             :") {
            homepage = Some(v.trim().to_string());
            in_dep = false;
            in_opt = false;
        } else if let Some(v) = line.strip_prefix("Installed Size  :") {
            size_install = Some(parse_size(v.trim()));
            in_dep = false;
            in_opt = false;
        } else if let Some(v) = line.strip_prefix("Download Size   :") {
            size_download = Some(parse_size(v.trim()));
            in_dep = false;
            in_opt = false;
        } else if let Some(v) = line.strip_prefix("Packager        :") {
            maintainer = Some(v.trim().to_string());
            in_dep = false;
            in_opt = false;
        } else if let Some(v) = line.strip_prefix("Description     :") {
            if summary.description.is_empty() {
                summary.description = v.trim().to_string();
            }
        } else if let Some(v) = line.strip_prefix("Version         :") {
            if summary.version.is_empty() {
                summary.version = v.trim().to_string();
            }
        } else if line.starts_with(char::is_whitespace) {
            // Continuations for previous key
            let v = line.trim();
            if in_dep && !v.is_empty() {
                depends.extend(parse_pkg_list(v));
            } else if in_opt && !v.is_empty() {
                if let Some(name) = v.split(':').next() {
                    let n = name.trim();
                    if !n.is_empty() {
                        opt_depends.push(n.to_string());
                    }
                }
            }
        } else {
            // New field
            in_dep = false;
            in_opt = false;
        }
    }

    PackageDetails {
        summary,
        depends,
        opt_depends,
        homepage,
        maintainer,
        size_install,
        size_download,
    }
}

fn parse_size(s: &str) -> u64 {
    let mut it = s.split_whitespace();
    let n: f64 = it.next().unwrap_or("0").parse().unwrap_or(0.0);
    match it.next().unwrap_or("B") {
        "KiB" => (n * 1024.0) as u64,
        "MiB" => (n * 1024.0 * 1024.0) as u64,
        "GiB" => (n * 1024.0 * 1024.0 * 1024.0) as u64,
        _ => n as u64,
    }
}

impl PacmanCli {
    fn run_stream(
        &self,
        mut cmd: Command,
        sink: &ProgressSink,
        cancel: &CancelToken,
        stage: Stage,
    ) -> Result<(i32, Option<String>)> {
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Internal(format!("spawn: {e}")))?;
        let out = child.stdout.take().expect("stdout should be piped");
        let err = child.stderr.take().expect("stderr should be piped");

        let jid = 0u64;
        let tx1 = sink.clone();
        let tx2 = sink.clone();

        let stage_out = stage.clone();
        let stage_err = stage;

        let last_err = Arc::new(Mutex::new(None::<String>));
        let last_err_t2 = last_err.clone();

        let t1 = std::thread::spawn(move || {
            for l in BufReader::new(out).lines().flatten() {
                let _ = tx1.send(Progress {
                    job_id: jid,
                    stage: stage_out.clone(),
                    percent: None,
                    bytes: None,
                    log: Some(l),
                    warning: false,
                });
            }
        });

        let t2 = std::thread::spawn(move || {
            for l in BufReader::new(err).lines().flatten() {
                {
                    let mut g = last_err_t2.lock();
                    *g = Some(l.clone());
                }
                let _ = tx2.send(Progress {
                    job_id: jid,
                    stage: stage_err.clone(),
                    percent: None,
                    bytes: None,
                    log: Some(l),
                    warning: true,
                });
            }
        });

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let _ = t1.join();
                    let _ = t2.join();
                    let code = status.code().unwrap_or(-1);
                    let last = last_err.lock().clone();
                    return Ok((code, last));
                }
                Ok(None) => {
                    if cancel.is_cancelled() {
                        #[cfg(unix)]
                        {
                            let _ = nix::sys::signal::kill(
                                nix::unistd::Pid::from_raw(child.id() as i32),
                                nix::sys::signal::Signal::SIGTERM,
                            );
                        }
                        let _ = child.wait();
                        let _ = t1.join();
                        let _ = t2.join();
                        return Err(Error::Cancelled);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(16));
                }
                Err(e) => return Err(Error::Internal(format!("wait: {e}"))),
            }
        }
    }
}

impl PackageBackend for PacmanCli {
    fn install(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let mut cmd = Command::new("pkexec");
        cmd.args(["pacman", "-S", "--noconfirm", "--needed", &id.name]);
        let (code, last_err) = self.run_stream(cmd, sink, cancel, Stage::Installing)?;
        if code == 0 {
            Ok(())
        } else {
            let why = last_err.map(|e| format!(": {e}")).unwrap_or_default();
            Err(Error::Priv(format!("install exit {code}{why}")))
        }
    }
    fn refresh(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let try_pkexec = || -> Result<()> {
            let mut cmd = Command::new("pkexec");
            cmd.args(["pacman", "-Sy", "--noconfirm"]);
            let (code, last_err) = self.run_stream(cmd, sink, cancel, Stage::Refreshing)?;
            if code == 0 {
                Ok(())
            } else {
                let why = last_err.map(|e| format!(": {e}")).unwrap_or_default();
                Err(Error::Alpm(format!("pacman -Sy exit {code}{why}")))
            }
        };

        match try_pkexec() {
            Ok(()) => Ok(()),
            Err(Error::Internal(e)) if e.contains("spawn:") => {
                // pkexec missing or cannot spawn. Explain and try unprivileged -Sy (may still fail).
                let _ = sink.send(Progress {
                    job_id: 0,
                    stage: Stage::Refreshing,
                    percent: None,
                    bytes: None,
                    log: Some(
                        "pkexec unavailable; attempting unprivileged refresh (may fail)".into(),
                    ),
                    warning: true,
                });
                let mut cmd = Command::new("pacman");
                cmd.args(["-Sy", "--noconfirm"]);
                let (code, last_err) = self.run_stream(cmd, sink, cancel, Stage::Refreshing)?;
                if code == 0 {
                    Ok(())
                } else {
                    let why = last_err.map(|e| format!(": {e}")).unwrap_or_default();
                    Err(Error::Alpm(format!("pacman -Sy exit {code}{why}")))
                }
            }
            Err(e) => Err(e),
        }
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
                log: Some("repo: query too short (<2), ignoring".into()),
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
            log: Some(format!("repo search: {q}")),
            warning: false,
        })
        .ok();

        // 1) Try -Ss first
        let out = match std::process::Command::new("pacman")
            .args(["-Ss", "--color", "never", q])
            .output()
        {
            Ok(o) => o,
            Err(e) => {
                sink.send(Progress {
                    job_id: 0,
                    stage: Stage::Searching,
                    percent: None,
                    bytes: None,
                    log: Some(format!(
                        "repo: failed to spawn pacman -Ss: {e} (falling back to -Ssq)"
                    )),
                    warning: true,
                })
                .ok();
                return self.search_fallback_names(q, sink);
            }
        };

        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();

        if out.status.success() {
            // Happy path
            return Ok(parse_pacman_search(&stdout));
        }

        // 2) Status != 0. If we still got lines on stdout, parse them.
        if !stdout.trim().is_empty() {
            sink.send(Progress {
                job_id: 0,
                stage: Stage::Searching,
                percent: None,
                bytes: None,
                log: Some(format!(
                    "repo: pacman -Ss exit {} but stdout has results; parsing anyway",
                    out.status.code().unwrap_or(-1)
                )),
                warning: true,
            })
            .ok();
            return Ok(parse_pacman_search(&stdout));
        }

        // stderr-only failure: explain and fall back to -Ssq
        let looks_like_db = stderr.contains("database")
            || stderr.contains("failed to synchronize")
            || stderr.contains("failed to update");
        let msg = if looks_like_db {
            "repo: pacman -Ss failed — repository database error. You can try Refresh (pacman -Sy) and search again."
            .to_string()
        } else {
            format!(
                "repo: pacman -Ss failed (exit {}): {}",
                out.status.code().unwrap_or(-1),
                stderr.trim()
            )
        };
        sink.send(Progress {
            job_id: 0,
            stage: Stage::Searching,
            percent: None,
            bytes: None,
            log: Some(msg + " (falling back to -Ssq)"),
            warning: true,
        })
        .ok();

        // 3) Fallback to -Ssq (names only)
        self.search_fallback_names(q, sink)
    }

    fn details(
        &self,
        id: &PackageId,
        _sink: &ProgressSink,
        _cancel: &CancelToken,
    ) -> Result<PackageDetails> {
        let out = Command::new("pacman")
            .args(["-Si", &id.name])
            .output()
            .map_err(|e| Error::Internal(e.to_string()))?;
        if !out.status.success() {
            return Err(Error::Alpm("pacman -Si failed".into()));
        }
        let s = String::from_utf8_lossy(&out.stdout);
        let summary = PackageSummary {
            id: id.clone(),
            version: String::new(),
            description: String::new(),
            installed: false,
            popular: None,
            last_updated: None,
        };
        Ok(parse_pacman_details(&s, summary))
    }

    fn remove(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let mut cmd = Command::new("pkexec");
        cmd.args(["pacman", "-Rns", "--noconfirm", &id.name]);
        let (code, last_err) = self.run_stream(cmd, sink, cancel, Stage::Removing)?;
        if code == 0 {
            Ok(())
        } else {
            let why = last_err.map(|e| format!(": {e}")).unwrap_or_default();
            Err(Error::Priv(format!("remove exit {code}{why}")))
        }
    }

    fn upgrades(&self, sink: &ProgressSink, _cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        // pacman -Qu does not require root and consults sync dbs for available updates
        let out = Command::new("pacman")
            .args(["-Qu", "--color", "never"])
            .output()
            .map_err(|e| Error::Internal(e.to_string()))?;

        if !out.status.success() && out.stdout.is_empty() {
            // Non-zero with no stdout usually means "no upgrades" or an error; treat as empty list.
            sink.send(Progress {
                job_id: 0,
                stage: Stage::Verifying,
                percent: None,
                bytes: None,
                log: Some(format!(
                    "repo: pacman -Qu exit {} (treating as no upgrades (non synced))",
                    out.status.code().unwrap_or(-1)
                )),
                warning: true,
            })
            .ok();
            return Ok(vec![]);
        }

        let stdout = String::from_utf8_lossy(&out.stdout);
        Ok(Self::parse_upgrades(&stdout))
    }

    fn upgrade(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        self.install(id, sink, cancel)
    }

    fn upgrade_all(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        // Full system upgrade, as pacman documents (-Syu).
        let mut cmd = Command::new("pkexec");
        cmd.args(["pacman", "-Syu", "--noconfirm"]);
        let (code, last_err) = self.run_stream(cmd, sink, cancel, Stage::Installing)?;
        if code == 0 {
            Ok(())
        } else {
            let why = last_err.map(|e| format!(": {e}")).unwrap_or_default();
            Err(Error::Priv(format!("upgrade-all exit {code}{why}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_upgrades_empty() {
        let r = PacmanCli::parse_upgrades("");
        assert!(r.is_empty());
    }

    #[test]
    fn test_parse_upgrades_single() {
        let r = PacmanCli::parse_upgrades("foo 1.0 -> 2.0\n");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id.name, "foo");
        assert_eq!(r[0].version, "2.0");
        assert_eq!(r[0].id.source, Source::Repo);
    }

    #[test]
    fn test_parse_upgrades_multiple() {
        let input = "a 1 -> 2\nb 3 -> 4\nc 5 -> 6\n";
        let r = PacmanCli::parse_upgrades(input);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].id.name, "a");
        assert_eq!(r[1].id.name, "b");
        assert_eq!(r[2].id.name, "c");
    }

    #[test]
    fn test_parse_pacman_search_empty() {
        let r = parse_pacman_search("");
        assert!(r.is_empty());
    }

    #[test]
    fn test_parse_pacman_search_single() {
        let input = "core/foo 1.0.0-1\n    A test package\n";
        let r = parse_pacman_search(input);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id.name, "foo");
        assert_eq!(r[0].version, "1.0.0-1");
        assert_eq!(r[0].description, "A test package");
    }

    #[test]
    fn test_parse_pacman_search_installed() {
        let input = "core/foo 1.0.0-1 [installed]\n    desc\n";
        let r = parse_pacman_search(input);
        assert_eq!(r.len(), 1);
        assert!(r[0].installed);
    }

    #[test]
    fn test_parse_pacman_search_not_installed() {
        let input = "core/foo 1.0.0-1\n    desc\n";
        let r = parse_pacman_search(input);
        assert_eq!(r.len(), 1);
        assert!(!r[0].installed);
    }

    #[test]
    fn test_parse_pacman_search_multiple() {
        let input = "core/a 1.0-1\n    pkg a\nextra/b 2.0-2 [installed]\n    pkg b\n";
        let r = parse_pacman_search(input);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].id.name, "a");
        assert!(!r[0].installed);
        assert_eq!(r[1].id.name, "b");
        assert!(r[1].installed);
    }

    #[test]
    fn test_parse_pacman_search_no_desc() {
        // A package header with no following description line
        let input = "core/foo 1.0-1\n";
        let r = parse_pacman_search(input);
        assert_eq!(r.len(), 1); // parsed with empty description
        assert_eq!(r[0].description, "");
    }

    #[test]
    fn test_parse_size_bytes() {
        assert_eq!(parse_size("42 B"), 42);
    }

    #[test]
    fn test_parse_size_kib() {
        assert_eq!(parse_size("1.5 KiB"), 1536);
    }

    #[test]
    fn test_parse_size_mib() {
        assert_eq!(parse_size("2 MiB"), 2 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_gib() {
        assert_eq!(parse_size("1 GiB"), 1024 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_default_unit() {
        assert_eq!(parse_size("100"), 100);
    }

    #[test]
    fn test_parse_size_zero() {
        assert_eq!(parse_size("0 B"), 0);
    }
}
