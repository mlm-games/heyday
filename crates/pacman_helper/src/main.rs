use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process;

use alpm::{Alpm, AnyQuestion, Question, SigLevel, TransFlag};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: pacman-helper <command> [args...]");
        process::exit(1);
    }

    let result = (|| -> Result<(), String> {
        let arch = std::env::consts::ARCH;
        let repos = parse_pacman_conf("/etc/pacman.conf", arch)?;

        let mut handle =
            Alpm::new("/", "/var/lib/pacman").map_err(|e| format!("alpm init: {e}"))?;

        handle
            .set_default_siglevel(SigLevel::USE_DEFAULT)
            .map_err(|e| format!("sig level: {e}"))?;

        handle.set_log_cb((), |level, msg, _| {
            let warning = level.contains(alpm::LogLevel::WARNING);
            emit_progress_stage("log", None, None, Some(msg.trim()), warning);
        });

        handle.set_progress_cb((), |progress, pkgname, percent, _howmany, _current, _| {
            let stage = match progress {
                alpm::Progress::AddStart
                | alpm::Progress::UpgradeStart
                | alpm::Progress::DowngradeStart
                | alpm::Progress::ReinstallStart => "installing",
                alpm::Progress::RemoveStart => "removing",
                alpm::Progress::ConflictsStart => "resolving",
                alpm::Progress::DiskspaceStart => "verifying",
                alpm::Progress::IntegrityStart => "verifying",
                alpm::Progress::LoadStart => "downloading",
                alpm::Progress::KeyringStart => "keyring",
            };
            let pct = if (0..=100).contains(&percent) {
                Some(percent as f64 / 100.0)
            } else {
                None
            };
            emit_progress_stage(stage, pct, None, Some(pkgname), false);
        });

        handle.set_event_cb((), |_event, _| {});

        handle.set_question_cb((), |question, _| {
            auto_answer(question);
        });

        for (name, servers) in &repos {
            let db = handle
                .register_syncdb_mut(name.as_str(), SigLevel::USE_DEFAULT)
                .map_err(|e| format!("register syncdb {name}: {e}"))?;
            for server in servers {
                if !server.is_empty() {
                    db.add_server(server.as_str())
                        .map_err(|e| format!("add server to {name}: {e}"))?;
                }
            }
        }

        let command = &args[1];
        match command.as_str() {
            "refresh" => cmd_refresh(&mut handle),
            "install" => {
                let pkgname = args.get(2).ok_or("missing package name")?;
                cmd_install(&mut handle, pkgname)
            }
            "remove" => {
                let pkgname = args.get(2).ok_or("missing package name")?;
                cmd_remove(&mut handle, pkgname)
            }
            "sysupgrade" => cmd_sysupgrade(&mut handle),
            "cache-clean" => {
                let keep = args
                    .get(2)
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(3);
                cmd_cache_clean(keep)
            }
            other => Err(format!("unknown command: {other}")),
        }
    })();

    match result {
        Ok(()) => {
            emit_json(r#"{"type":"result","code":0}"#);
            process::exit(0);
        }
        Err(e) => {
            emit_json(&format!(
                r#"{{"type":"result","code":1,"message":"{}"}}"#,
                escape_json(&e)
            ));
            process::exit(1);
        }
    }
}

fn parse_pacman_conf(path: &str, arch: &str) -> Result<HashMap<String, Vec<String>>, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("reading {path}: {e}"))?;
    let mut repos: HashMap<String, Vec<String>> = HashMap::new();
    let mut current_repo: Option<String> = None;
    let mut current_servers: Vec<String> = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            if let Some(name) = current_repo.take() {
                if name != "options" {
                    repos.insert(name, std::mem::take(&mut current_servers));
                }
                current_servers.clear();
            }
            let name = line[1..line.len() - 1].to_lowercase();
            current_repo = Some(name);
        } else if let Some(ref name) = current_repo {
            if let Some(server) = line.strip_prefix("Server = ") {
                let resolved = server.replace("$repo", name).replace("$arch", arch);
                current_servers.push(resolved);
            } else if let Some(include_path) = line.strip_prefix("Include = ")
                && let Ok(include_content) = fs::read_to_string(include_path)
            {
                for iline in include_content.lines() {
                    let iline = iline.trim();
                    if let Some(server) = iline.strip_prefix("Server = ") {
                        let resolved = server.replace("$repo", name).replace("$arch", arch);
                        current_servers.push(resolved);
                    }
                }
            }
        }
    }

    if let Some(name) = current_repo
        && name != "options"
    {
        repos.insert(name, current_servers);
    }

    Ok(repos)
}

fn cmd_refresh(handle: &mut Alpm) -> Result<(), String> {
    let dbs = handle.syncdbs_mut();
    if dbs.is_empty() {
        return Err("no sync databases registered".into());
    }
    dbs.update(true).map_err(|e| format!("refresh: {e}"))?;
    Ok(())
}

fn cmd_install(handle: &mut Alpm, pkgname: &str) -> Result<(), String> {
    let mut pkg = None;
    for db in handle.syncdbs() {
        if let Ok(p) = db.pkg(pkgname) {
            pkg = Some(p);
            break;
        }
    }
    let pkg = pkg.ok_or_else(|| format!("package not found in repos: {pkgname}"))?;

    let flags = TransFlag::NEEDED;
    handle
        .trans_init(flags)
        .map_err(|e| format!("trans_init: {e}"))?;

    let r = handle
        .trans_add_pkg(pkg)
        .map_err(|e| format!("add pkg: {}", e.error));
    let r = r.and_then(|_| {
        handle
            .trans_prepare()
            .map_err(|e| format!("prepare: {}", e.error()))
    });
    let r = r.and_then(|_| {
        handle
            .trans_commit()
            .map_err(|e| format!("commit: {}", e.error()))
    });

    handle.trans_release().ok();
    r
}

fn cmd_remove(handle: &mut Alpm, pkgname: &str) -> Result<(), String> {
    let pkg = handle
        .localdb()
        .pkg(pkgname)
        .map_err(|_| format!("package not installed: {pkgname}"))?;

    let flags = TransFlag::CASCADE | TransFlag::RECURSE;
    handle
        .trans_init(flags)
        .map_err(|e| format!("trans_init: {e}"))?;

    let r = handle
        .trans_remove_pkg(pkg)
        .map_err(|e| format!("remove pkg: {e}"));
    let r = r.and_then(|_| {
        handle
            .trans_prepare()
            .map_err(|e| format!("prepare: {}", e.error()))
    });
    let r = r.and_then(|_| {
        handle
            .trans_commit()
            .map_err(|e| format!("commit: {}", e.error()))
    });

    handle.trans_release().ok();
    r
}

fn cmd_sysupgrade(handle: &mut Alpm) -> Result<(), String> {
    {
        let dbs = handle.syncdbs_mut();
        if !dbs.is_empty() {
            dbs.update(true).map_err(|e| format!("refresh: {e}"))?;
        }
    }

    let flags = TransFlag::NONE;
    handle
        .trans_init(flags)
        .map_err(|e| format!("trans_init: {e}"))?;

    let r = handle
        .sync_sysupgrade(false)
        .map_err(|e| format!("sysupgrade: {e}"));
    let r = r.and_then(|_| {
        handle
            .trans_prepare()
            .map_err(|e| format!("prepare: {}", e.error()))
    });
    let r = r.and_then(|_| {
        handle
            .trans_commit()
            .map_err(|e| format!("commit: {}", e.error()))
    });

    handle.trans_release().ok();
    r
}

const PACMAN_CACHE: &str = "/var/cache/pacman/pkg";

fn cmd_cache_clean(keep: u32) -> Result<(), String> {
    let dir = Path::new(PACMAN_CACHE);
    let mut entries: Vec<(String, std::path::PathBuf)> = Vec::new();
    let read_dir = fs::read_dir(dir).map_err(|e| format!("read cache dir: {e}"))?;
    for entry in read_dir.flatten() {
        let fname = entry.file_name().to_string_lossy().to_string();
        if fname.ends_with(".pkg.tar.zst") {
            entries.push((fname, entry.path()));
        }
    }

    // Group by package name (strip everything after first '-' after the name prefix)
    let mut groups: HashMap<String, Vec<(String, std::path::PathBuf)>> = HashMap::new();
    for (fname, path) in entries {
        let pkg_name = fname
            .rsplitn(2, '-')
            .nth(1)
            .and_then(|s| s.rsplitn(2, '-').nth(1))
            .unwrap_or(&fname)
            .to_string();
        groups.entry(pkg_name).or_default().push((fname, path));
    }

    let mut removed = 0u32;
    for (_name, mut pkgs) in groups {
        if pkgs.len() <= keep as usize {
            continue;
        }
        // Sort by version descending (simple string comparison, good enough for cache clean)
        pkgs.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, p) in pkgs.iter().skip(keep as usize) {
            if fs::remove_file(p).is_ok() {
                removed += 1;
            }
        }
    }

    emit_progress_stage("cleaning", None, None, Some(&format!("removed {removed} cached files")), false);
    Ok(())
}

fn auto_answer(mut question: AnyQuestion) {
    match question.question() {
        Question::SelectProvider(ref mut q) => q.set_index(0),
        _ => question.set_answer(true),
    }
}

fn emit_json(data: &str) {
    println!("{data}");
    io::stdout().flush().ok();
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn emit_progress_stage(
    stage: &str,
    percent: Option<f64>,
    bytes: Option<(u64, u64)>,
    log: Option<&str>,
    warning: bool,
) {
    let mut parts = vec![format!(r#""type":"progress","stage":"{stage}""#)];
    if let Some(p) = percent {
        parts.push(format!(r#""percent":{p}"#));
    }
    if let Some((cur, tot)) = bytes {
        parts.push(format!(r#""bytes":[{cur},{tot}]"#));
    }
    if let Some(l) = log {
        parts.push(format!(r#""log":"{}""#, escape_json(l)));
    }
    parts.push(format!(r#""warning":{warning}"#));
    emit_json(&format!("{{{}}}", parts.join(",")));
}
