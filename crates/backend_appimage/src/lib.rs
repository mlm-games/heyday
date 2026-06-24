use domain::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AppImageEntry {
    path: String,
    version: String,
    name: String,
    desktop_file: Option<String>,
    download_url: Option<String>,
    update_url: Option<String>,
    update_pattern: Option<String>,
    local_size: u64,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    developer: Option<String>,
    #[serde(default)]
    homepage: Option<String>,
    #[serde(default)]
    long_description: Option<String>,
}

type Config = HashMap<String, AppImageEntry>;

fn config_path() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!("{home}/.config/soredowe/appimages.json")
}

fn read_config() -> Config {
    let path = config_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_config(cfg: &Config) {
    let path = config_path();
    if let Some(parent) = Path::new(&path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string_pretty(cfg) {
        let _ = fs::write(&path, s);
    }
}

fn user_apps_dir() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!("{home}/Applications")
}

fn user_desktop_files_dir() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!("{home}/.local/share/applications")
}

fn send_log(sink: &ProgressSink, stage: Stage, msg: &str, warning: bool) {
    let _ = sink.send(Progress {
        job_id: 0,
        stage,
        percent: None,
        bytes: None,
        log: Some(msg.into()),
        warning,
    });
}

fn find_desktop_file(dir: &Path) -> Option<String> {
    for entry in fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("desktop") {
            return path.to_str().map(|s| s.to_string());
        }
    }
    None
}

fn try_extract_7z(appimage: &str, tmpdir: &str, bin: &str) -> Option<std::process::Output> {
    std::process::Command::new(bin)
        .args([
            "x",
            appimage,
            &format!("-o{tmpdir}"),
            "*.desktop",
            "-y",
            "-aoa",
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
}

fn extract_metainfo(appimage: &str) -> Option<domain::appstream::AppstreamMeta> {
    let work = tempfile::tempdir().ok()?;
    let out_dir = work.path().to_str()?;
    for bin in &["7zz", "7z"] {
        let output = std::process::Command::new(bin)
            .args([
                "x",
                appimage,
                &format!("-o{out_dir}"),
                "-ir!usr/share/metainfo/",
                "-ir!usr/share/appdata/",
                "-y",
                "-aoa",
            ])
            .output()
            .ok()
            .filter(|o| o.status.success());
        if output.is_some() {
            break;
        }
    }

    let found = find_metainfo_files(work.path()).into_iter().find_map(|p| {
        let file = fs::File::open(&p).ok()?;
        let map = domain::appstream::parse_appstream_xml(file);
        map.into_values().next()
    });
    found
}

fn find_metainfo_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(find_metainfo_files(&path));
            } else if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if name.ends_with(".metainfo.xml") || name.ends_with(".appdata.xml") {
                    files.push(path);
                }
            }
        }
    }
    files
}

fn extract_desktop_entry(appimage: &str) -> Option<(String, String)> {
    let work = tempfile::tempdir().ok()?;
    let tmpdir = work.path().to_str()?.to_string();

    // 7zz (full 7-Zip) and 7z (p7zip) both support Zstd needed for AppImage.
    // 7za (standalone p7zip) does NOT support Zstd — skip it.
    let ok = try_extract_7z(appimage, &tmpdir, "7zz")
        .or_else(|| try_extract_7z(appimage, &tmpdir, "7z"))
        .or_else(|| {
            std::process::Command::new("unsquashfs")
                .args(["-d", &tmpdir, "-f", appimage])
                .output()
                .ok()
                .filter(|o| o.status.success())
        })
        .or_else(|| {
            // Fallback: --appimage-extract (like gearlever)
            let cloned = format!("{tmpdir}/app.AppImage");
            fs::copy(appimage, &cloned).ok()?;
            let _ =
                fs::set_permissions(&cloned, std::os::unix::fs::PermissionsExt::from_mode(0o755));
            let out = std::process::Command::new(&cloned)
                .args(["--appimage-extract"])
                .current_dir(&tmpdir)
                .output()
                .ok()?;
            if out.status.success() {
                Some(out)
            } else {
                None
            }
        })
        .is_some();

    if !ok {
        return None;
    }

    // --appimage-extract creates squashfs-root/, other methods extract to tmpdir root
    let search_dir = if Path::new(&format!("{tmpdir}/squashfs-root")).exists() {
        format!("{tmpdir}/squashfs-root")
    } else {
        tmpdir
    };

    let desktop_path = find_desktop_file(Path::new(&search_dir))?;
    let content = fs::read_to_string(&desktop_path).ok()?;

    let app_name = content
        .lines()
        .find(|l| l.starts_with("Name="))
        .and_then(|l| l.strip_prefix("Name="))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if app_name.is_empty() {
        return None;
    }

    Some((app_name, content))
}

/// Extract executable and arguments from a desktop file Exec line,
/// matching gearlever's `extract_terminal_arguments`.
fn parse_exec(value: &str) -> (Vec<String>, Vec<String>) {
    // Returns (env_vars, args_without_executable)
    // env_vars: tokens like KEY=VALUE preceding the executable
    // args_without_executable: all tokens after the executable, minus field codes
    let tokens = shlex::split(value).unwrap_or_default();
    let mut env_vars: Vec<String> = Vec::new();
    let mut args: Vec<String> = Vec::new();
    let mut found_exec = false;
    let mut i = 0;
    while i < tokens.len() {
        let t = &tokens[i];
        if t == "env" {
            i += 1;
            continue;
        }
        if t.contains('=') && !t.starts_with('/') && !t.starts_with('-') {
            env_vars.push(t.clone());
        } else if !found_exec {
            // this is the executable itself — skip it
            found_exec = true;
        } else {
            args.push(t.clone());
        }
        i += 1;
    }
    (env_vars, args)
}

fn write_desktop_file(
    raw_content: &str,
    appimage_path: &str,
    app_name: &str,
    version: Option<&str>,
) -> Option<String> {
    let dest_dir = user_desktop_files_dir();
    let dest = format!("{dest_dir}/{app_name}.desktop");
    let _ = fs::create_dir_all(&dest_dir);

    let mut lines: Vec<String> = Vec::new();
    let mut in_desktop_entry = true;
    let mut wrote_exec = false;
    let mut wrote_tryexec = false;
    for line in raw_content.lines() {
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line.eq_ignore_ascii_case("[Desktop Entry]");
        }
        if in_desktop_entry {
            if line.starts_with("Exec=") {
                let rest = line.strip_prefix("Exec=").unwrap_or("");
                let (env_vars, mut args) = parse_exec(rest);
                args.retain(|s| {
                    !matches!(s.as_str(), "%f" | "%F" | "%u" | "%U" | "%i" | "%c" | "%k")
                });

                let mut all: Vec<&str> = Vec::new();
                // env vars first (gearlever adds them via update_desktop_file, but
                // we add DESKTOPINTEGRATION directly here)
                all.push("env");
                all.push("DESKTOPINTEGRATION=1");
                for ev in &env_vars {
                    all.push(ev);
                }
                all.push(appimage_path);
                for a in &args {
                    all.push(a);
                }
                let cmd = shlex::try_join(all.iter().copied()).unwrap_or_default();
                lines.push(format!("Exec={cmd}"));
                wrote_exec = true;
                continue;
            }
            if line.starts_with("TryExec=") {
                lines.push(format!("TryExec={appimage_path}"));
                wrote_tryexec = true;
                continue;
            }
        }
        lines.push(line.to_string());
    }
    if !wrote_tryexec {
        lines.push(format!("TryExec={appimage_path}"));
    }
    if !wrote_exec {
        let cmd = shlex::try_join(["env", "DESKTOPINTEGRATION=1", appimage_path].into_iter())
            .unwrap_or_default();
        lines.push(format!("Exec={cmd}"));
    }
    if let Some(ver) = version {
        lines.push(format!("X-AppImage-Version={ver}"));
    }

    let out = lines.join("\n");
    fs::write(&dest, &out).ok()?;
    Some(dest)
}

fn extract_upd_info(appimage: &str) -> Option<(String, String)> {
    let output = std::process::Command::new("readelf")
        .args(["-p", ".upd_info", "--wide", appimage])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);

    // readelf outputs: "String dump of section '.upd_info':\n  [     0]  gh-releases-zsync|..."
    // Search for the pattern anywhere in the output (like gearlever does)
    let all = stdout.replace('\n', " ") + " ";

    // gh-releases / gh-releases-zsync format
    if let Some(start) = all.find("gh-releases") {
        let rest = &all[start..];
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let token = &rest[..end];
        let parts: Vec<&str> = token.split('|').collect();
        if parts.len() >= 5 {
            let owner = parts[1];
            let repo = parts[2];
            let tag = parts[3];
            let pattern = parts[4];
            let api_url = format!("https://api.github.com/repos/{owner}/{repo}/releases/{tag}");
            return Some((api_url, pattern.to_string()));
        }
    }

    // zsync|http... format
    if let Some(start) = all.find("zsync|http") {
        let rest = &all[start + 6..];
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let url = rest[..end].to_string();
        if url.starts_with("http") {
            return Some((url, String::new()));
        }
    }

    None
}

pub struct AppImageBackend;

impl AppImageBackend {
    pub fn new() -> Self {
        Self
    }
}

impl PackageBackend for AppImageBackend {
    fn name(&self) -> &'static str {
        "appimage"
    }

    fn refresh(&self, _sink: &ProgressSink, _cancel: &CancelToken) -> Result<()> {
        Ok(())
    }

    fn search(
        &self,
        q: &str,
        _sink: &ProgressSink,
        _cancel: &CancelToken,
    ) -> Result<Vec<PackageSummary>> {
        let q = q.to_lowercase();
        Ok(self
            .installed(_cancel)?
            .into_iter()
            .filter(|p| p.id.name.to_lowercase().contains(&q))
            .collect())
    }

    fn details(
        &self,
        id: &PackageId,
        _sink: &ProgressSink,
        _cancel: &CancelToken,
    ) -> Result<PackageDetails> {
        let cfg = read_config();
        let entry = cfg.get(&id.name);
        let path = entry
            .map(|e| e.path.clone())
            .or_else(|| {
                let dir = user_apps_dir();
                let p = format!("{dir}/{}.AppImage", id.name);
                if Path::new(&p).exists() {
                    Some(p)
                } else {
                    None
                }
            })
            .ok_or_else(|| Error::AppImage(format!("{} not found", id.name)))?;

        let meta = fs::metadata(&path).map_err(|e| Error::AppImage(e.to_string()))?;

        let desktop_content = entry
            .and_then(|e| e.desktop_file.as_deref())
            .and_then(|p| fs::read_to_string(p).ok())
            .or_else(|| {
                let p = format!("{}/{}.desktop", user_desktop_files_dir(), id.name);
                fs::read_to_string(p).ok()
            });
        let (version, description) = match desktop_content {
            Some(content) => (
                content
                    .lines()
                    .find(|l| l.starts_with("X-AppImage-Version="))
                    .and_then(|l| l.strip_prefix("X-AppImage-Version="))
                    .map(|s| s.trim().to_string())
                    .or_else(|| entry.map(|e| e.version.clone()))
                    .unwrap_or_default(),
                content
                    .lines()
                    .find(|l| l.starts_with("Comment="))
                    .and_then(|l| l.strip_prefix("Comment="))
                    .map(|s| s.trim().to_string()),
            ),
            None => (entry.map(|e| e.version.clone()).unwrap_or_default(), None),
        };

        Ok(PackageDetails {
            summary: PackageSummary {
                id: id.clone(),
                version,
                description: description.clone().unwrap_or_default(),
                installed: true,
                popular: None,
                last_updated: meta.modified().ok(),
            },
            description: description.or_else(|| entry.and_then(|e| e.long_description.clone())),
            depends: vec![],
            opt_depends: vec![],
            homepage: entry.and_then(|e| e.homepage.clone()),
            license: entry.and_then(|e| e.license.clone()),
            maintainer: None,
            developer: entry.and_then(|e| e.developer.clone()),
            size_install: Some(meta.len()),
            size_download: None,
        })
    }

    fn installed(&self, _cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        let mut results = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let desk_dir = user_desktop_files_dir();
        let apps_dir = user_apps_dir();

        // Scan desktop files (primary source, like gearlever)
        if let Ok(entries) = fs::read_dir(&desk_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
                    continue;
                }
                let content = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    _ => continue,
                };

                let try_exec = content
                    .lines()
                    .find(|l| l.starts_with("TryExec="))
                    .and_then(|l| l.strip_prefix("TryExec="))
                    .map(|s| s.trim().to_string());

                let exec_path = match try_exec.as_deref().and_then(|p| {
                    if Path::new(p).exists() {
                        Some(p.to_string())
                    } else {
                        None
                    }
                }) {
                    Some(p) => p,
                    None => continue,
                };

                // Verify it's actually an AppImage
                let ext = Path::new(&exec_path)
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                if !ext.eq_ignore_ascii_case("AppImage") && !ext.eq_ignore_ascii_case("appimage") {
                    continue;
                }

                let app_name = content
                    .lines()
                    .find(|l| l.starts_with("Name="))
                    .and_then(|l| l.strip_prefix("Name="))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                if app_name.is_empty() || !seen.insert(app_name.clone()) {
                    continue;
                }

                let version = content
                    .lines()
                    .find(|l| l.starts_with("X-AppImage-Version="))
                    .and_then(|l| l.strip_prefix("X-AppImage-Version="))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();

                let description = content
                    .lines()
                    .find(|l| l.starts_with("Comment="))
                    .and_then(|l| l.strip_prefix("Comment="))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();

                let meta = fs::metadata(&exec_path).ok();
                results.push(PackageSummary {
                    id: PackageId {
                        name: app_name,
                        source: Source::AppImage,
                        repo: None,
                    },
                    version,
                    description,
                    installed: true,
                    popular: None,
                    last_updated: meta.and_then(|m| m.modified().ok()),
                });
            }
        }

        // Fallback: scan Apps directory for unregistered AppImages
        let cfg = read_config();
        if let Ok(entries) = fs::read_dir(&apps_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                if !ext.eq_ignore_ascii_case("AppImage") && !ext.eq_ignore_ascii_case("appimage") {
                    continue;
                }
                let path_str = path.to_string_lossy().to_string();
                let config_entry = cfg.iter().find(|(_, e)| e.path == path_str);
                let app_name = config_entry.map(|(n, _)| n.clone()).unwrap_or_else(|| {
                    path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string()
                });
                if app_name.is_empty() || !seen.insert(app_name.clone()) {
                    continue;
                }
                let meta = fs::metadata(&path).ok();
                let version = config_entry
                    .map(|(_, e)| e.version.clone())
                    .unwrap_or_default();
                results.push(PackageSummary {
                    id: PackageId {
                        name: app_name,
                        source: Source::AppImage,
                        repo: None,
                    },
                    version,
                    description: String::new(),
                    installed: true,
                    popular: None,
                    last_updated: meta.and_then(|m| m.modified().ok()),
                });
            }
        }

        Ok(results)
    }

    fn updates(&self, _cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        let cfg = read_config();
        let mut results = Vec::new();
        let agent = ureq::Agent::new_with_defaults();
        for (_key, entry) in &cfg {
            // Need an update_url (GitHub API URL) to check for updates
            let Some(ref api_url) = entry.update_url else {
                continue;
            };
            let pattern = entry.update_pattern.as_deref();

            let Some((tag, remote_size, download_url)) =
                check_gh_update(&agent, api_url, pattern, &entry.name)
            else {
                continue;
            };

            if remote_size == entry.local_size {
                continue;
            }

            results.push(PackageSummary {
                id: PackageId {
                    name: entry.name.clone(),
                    source: Source::AppImage,
                    repo: Some(download_url),
                },
                version: tag,
                description: String::new(),
                installed: true,
                popular: None,
                last_updated: None,
            });
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
            OperationKind::Refresh => Ok(()),
        }
    }

    fn install(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let url = id
            .repo
            .as_deref()
            .ok_or_else(|| Error::AppImage("no download URL".into()))?;

        let dir = user_apps_dir();
        let _ = fs::create_dir_all(&dir);

        let dest = format!("{dir}/{}.AppImage", id.name);
        let tmp = format!("{dir}/.{}.AppImage.part", id.name);

        let agent = ureq::Agent::new_with_defaults();
        let resp = agent
            .get(url)
            .call()
            .map_err(|e: ureq::Error| Error::AppImage(format!("download failed: {e}")))?;

        let total = resp.body().content_length().unwrap_or(0);

        let mut reader = resp.into_body().into_reader();
        let mut file = File::create(&tmp).map_err(|e| Error::AppImage(e.to_string()))?;
        let mut buf = [0u8; 65536];
        let mut downloaded = 0u64;

        loop {
            if cancel.is_cancelled() {
                let _ = fs::remove_file(&tmp);
                return Err(Error::Cancelled);
            }
            let n = reader
                .read(&mut buf)
                .map_err(|e| Error::AppImage(e.to_string()))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])
                .map_err(|e| Error::AppImage(e.to_string()))?;
            downloaded += n as u64;
            if total > 0 {
                let pct = downloaded as f32 / total as f32;
                let _ = sink.send(Progress {
                    job_id: 0,
                    stage: Stage::Downloading,
                    percent: Some(pct),
                    bytes: Some((downloaded, total)),
                    log: None,
                    warning: false,
                });
            }
        }

        drop(file);
        fs::rename(&tmp, &dest).map_err(|e| Error::AppImage(e.to_string()))?;
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))
            .map_err(|e| Error::AppImage(e.to_string()))?;

        let local_size = fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);

        let (app_name, version, desktop_file) =
            if let Some((an, body)) = extract_desktop_entry(&dest) {
                let ver = body
                    .lines()
                    .find(|l| l.starts_with("X-AppImage-Version="))
                    .and_then(|l| l.strip_prefix("X-AppImage-Version="))
                    .map(|s| s.trim().to_string());
                let df = write_desktop_file(&body, &dest, &an, ver.as_deref());
                (an, ver, df)
            } else {
                (id.name.clone(), None, None)
            };

        let (upd_api_url, upd_pattern) = extract_upd_info(&dest).unzip();

        let meta = extract_metainfo(&dest);
        let ver = version.or_else(|| meta.as_ref().and_then(|m| m.version.clone()));

        let mut cfg = read_config();
        cfg.insert(
            app_name.clone(),
            AppImageEntry {
                path: dest,
                version: ver.unwrap_or_default(),
                name: app_name.clone(),
                desktop_file,
                download_url: Some(url.to_string()),
                update_url: upd_api_url,
                update_pattern: upd_pattern,
                local_size,
                license: meta.as_ref().and_then(|m| m.license.clone()),
                developer: meta.as_ref().and_then(|m| m.developer.clone()),
                homepage: meta.as_ref().and_then(|m| m.homepage.clone()),
                long_description: meta.as_ref().and_then(|m| {
                    if m.description.is_empty() {
                        None
                    } else {
                        Some(m.description.clone())
                    }
                }),
            },
        );
        write_config(&cfg);

        let _ = std::process::Command::new("update-desktop-database")
            .args([&user_desktop_files_dir(), "-q"])
            .output();

        Ok(())
    }

    fn install_file(&self, path: &str, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let src = Path::new(path);
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        let app_dir = user_apps_dir();
        let _ = fs::create_dir_all(&app_dir);

        // Get app name from desktop entry first, fallback to filename
        let (app_name, desktop_body) = extract_desktop_entry(path).unwrap_or_else(|| {
            let stem = src
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            (stem, String::new())
        });

        let dest = format!("{app_dir}/{app_name}.AppImage");

        send_log(
            sink,
            Stage::Installing,
            &format!("copying {app_name}..."),
            false,
        );
        fs::copy(src, &dest).map_err(|e| Error::AppImage(format!("copy: {e}")))?;
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))
            .map_err(|e| Error::AppImage(e.to_string()))?;

        let (upd_api_url, upd_pattern) = extract_upd_info(&dest).unzip();
        let local_size = fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);

        let version = desktop_body
            .lines()
            .find(|l| l.starts_with("X-AppImage-Version="))
            .and_then(|l| l.strip_prefix("X-AppImage-Version="))
            .map(|s| s.trim().to_string());

        let desktop_file = if desktop_body.is_empty() {
            None
        } else {
            write_desktop_file(&desktop_body, &dest, &app_name, version.as_deref())
        };

        let download_url = None; // local file, no download URL
        let meta = extract_metainfo(&dest);
        let ver = version.or_else(|| meta.as_ref().and_then(|m| m.version.clone()));

        let mut cfg = read_config();
        cfg.insert(
            app_name.clone(),
            AppImageEntry {
                path: dest,
                version: ver.unwrap_or_default(),
                name: app_name.clone(),
                desktop_file,
                download_url,
                update_url: upd_api_url,
                update_pattern: upd_pattern,
                local_size,
                license: meta.as_ref().and_then(|m| m.license.clone()),
                developer: meta.as_ref().and_then(|m| m.developer.clone()),
                homepage: meta.as_ref().and_then(|m| m.homepage.clone()),
                long_description: meta.as_ref().and_then(|m| {
                    if m.description.is_empty() {
                        None
                    } else {
                        Some(m.description.clone())
                    }
                }),
            },
        );
        write_config(&cfg);

        // Refresh desktop database
        let _ = std::process::Command::new("update-desktop-database")
            .args([&user_desktop_files_dir(), "-q"])
            .output();

        send_log(
            sink,
            Stage::Finished,
            &format!("{app_name} installed as AppImage"),
            false,
        );
        Ok(())
    }

    fn remove(&self, id: &PackageId, _sink: &ProgressSink, _cancel: &CancelToken) -> Result<()> {
        let cfg = read_config();
        let path = cfg
            .get(&id.name)
            .map(|e| e.path.clone())
            .or_else(|| {
                let p = format!("{}/{}.AppImage", user_apps_dir(), id.name);
                if Path::new(&p).exists() {
                    Some(p)
                } else {
                    None
                }
            })
            .ok_or_else(|| Error::AppImage(format!("{} not found", id.name)))?;

        let _ = fs::remove_file(&path);

        // Remove desktop file — use stored path or derive from name
        let desktop_path = cfg
            .get(&id.name)
            .and_then(|e| e.desktop_file.as_deref())
            .map(|s| s.to_string());
        let desktop = desktop_path
            .unwrap_or_else(|| format!("{}/{}.desktop", user_desktop_files_dir(), id.name));
        let _ = fs::remove_file(&desktop);

        let mut c = read_config();
        c.remove(&id.name);
        write_config(&c);

        let _ = std::process::Command::new("update-desktop-database")
            .args([&user_desktop_files_dir(), "-q"])
            .output();

        Ok(())
    }

    fn upgrade(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let cfg = read_config();
        let url = cfg
            .get(&id.name)
            .and_then(|e| e.download_url.as_deref())
            .map(|s| s.to_string())
            .or_else(|| id.repo.clone())
            .ok_or_else(|| Error::AppImage("no update URL".into()))?;

        let dir = user_apps_dir();
        let _ = fs::create_dir_all(&dir);
        let dest = format!("{dir}/{}.AppImage", id.name);
        let tmp = format!("{dir}/.{}.AppImage.new", id.name);

        let agent = ureq::Agent::new_with_defaults();
        let resp = agent
            .get(&url)
            .call()
            .map_err(|e: ureq::Error| Error::AppImage(format!("download failed: {e}")))?;
        let total = resp.body().content_length().unwrap_or(0);

        let mut reader = resp.into_body().into_reader();
        let mut file = File::create(&tmp).map_err(|e| Error::AppImage(e.to_string()))?;
        let mut buf = [0u8; 65536];
        let mut downloaded = 0u64;

        loop {
            if cancel.is_cancelled() {
                let _ = fs::remove_file(&tmp);
                return Err(Error::Cancelled);
            }
            let n = reader
                .read(&mut buf)
                .map_err(|e| Error::AppImage(e.to_string()))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])
                .map_err(|e| Error::AppImage(e.to_string()))?;
            downloaded += n as u64;
            if total > 0 {
                let pct = downloaded as f32 / total as f32;
                let _ = sink.send(Progress {
                    job_id: 0,
                    stage: Stage::Downloading,
                    percent: Some(pct),
                    bytes: Some((downloaded, total)),
                    log: None,
                    warning: false,
                });
            }
        }
        drop(file);

        // Remove old before replacing
        let old_path = cfg.get(&id.name).map(|e| e.path.clone());
        if let Some(ref old) = old_path {
            let _ = fs::remove_file(old);
        }
        let desktop_path = cfg
            .get(&id.name)
            .and_then(|e| e.desktop_file.as_deref())
            .map(|s| s.to_string());
        if let Some(ref dp) = desktop_path {
            let _ = fs::remove_file(dp);
        }

        fs::rename(&tmp, &dest).map_err(|e| Error::AppImage(e.to_string()))?;
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))
            .map_err(|e| Error::AppImage(e.to_string()))?;

        let local_size = fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
        let (app_name, version, desktop_file) =
            if let Some((an, body)) = extract_desktop_entry(&dest) {
                let ver = body
                    .lines()
                    .find(|l| l.starts_with("X-AppImage-Version="))
                    .and_then(|l| l.strip_prefix("X-AppImage-Version="))
                    .map(|s| s.trim().to_string());
                let df = write_desktop_file(&body, &dest, &an, ver.as_deref());
                (an, ver, df)
            } else {
                (id.name.clone(), None, None)
            };

        let (upd_api_url, upd_pattern) = extract_upd_info(&dest).unzip();
        let meta = extract_metainfo(&dest);
        let ver = version.or_else(|| meta.as_ref().and_then(|m| m.version.clone()));

        let mut cfg = read_config();
        cfg.remove(&id.name);
        cfg.insert(
            app_name.clone(),
            AppImageEntry {
                path: dest,
                version: ver.unwrap_or_default(),
                name: app_name,
                desktop_file,
                download_url: Some(url),
                update_url: upd_api_url,
                update_pattern: upd_pattern,
                local_size,
                license: meta.as_ref().and_then(|m| m.license.clone()),
                developer: meta.as_ref().and_then(|m| m.developer.clone()),
                homepage: meta.as_ref().and_then(|m| m.homepage.clone()),
                long_description: meta.as_ref().and_then(|m| {
                    if m.description.is_empty() {
                        None
                    } else {
                        Some(m.description.clone())
                    }
                }),
            },
        );
        write_config(&cfg);

        let _ = std::process::Command::new("update-desktop-database")
            .args([&user_desktop_files_dir(), "-q"])
            .output();

        Ok(())
    }

    fn upgrade_all(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let installed = self.installed(cancel)?;
        for pkg in &installed {
            let cfg = read_config();
            if cfg
                .get(&pkg.id.name)
                .and_then(|e| e.download_url.as_ref())
                .is_some()
            {
                self.upgrade(&pkg.id, sink, cancel)?;
            }
        }
        Ok(())
    }
}

fn fnmatch(pattern: &str, name: &str) -> bool {
    let mut pi = 0usize;
    let mut ni = 0usize;
    let pbytes = pattern.as_bytes();
    let nbytes = name.as_bytes();
    let plen = pbytes.len();
    let nlen = nbytes.len();
    let mut backtrack_pi = plen;
    let mut backtrack_ni = 0usize;

    while ni < nlen {
        if pi < plen && pbytes[pi] == b'*' {
            backtrack_pi = pi;
            backtrack_ni = ni + 1;
            pi += 1;
        } else if pi < plen && (pbytes[pi] == b'?' || pbytes[pi] == nbytes[ni]) {
            pi += 1;
            ni += 1;
        } else {
            if backtrack_pi < plen {
                pi = backtrack_pi;
                ni = backtrack_ni;
                backtrack_pi = plen;
                backtrack_ni = 0;
                continue;
            }
            return false;
        }
    }
    while pi < plen && pbytes[pi] == b'*' {
        pi += 1;
    }
    pi == plen
}

fn extract_json_str<'a>(obj: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\":\"");
    let start = obj.find(&needle)?;
    let rest = &obj[start + needle.len()..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn check_gh_update(
    agent: &ureq::Agent,
    url: &str,
    pattern: Option<&str>,
    name: &str,
) -> Option<(String, u64, String)> {
    if !url.starts_with("https://api.github.com/") {
        return None;
    }

    let mut resp = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "soredowe/0.4.0")
        .call()
        .ok()?;
    if resp.status() != 200 {
        return None;
    }
    let body = resp.body_mut().read_to_string().ok()?;

    let tag = extract_json_str(&body, "tag_name")?.to_string();

    let assets_key = "\"assets\":[";
    let assets_start = body.find(assets_key)?;
    let assets_rest = &body[assets_start + assets_key.len()..];

    let mut depth = 1u32;
    let mut i = 0;
    let bytes = assets_rest.as_bytes();
    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    let assets_json = &assets_rest[..i];

    let name_lower = name.to_lowercase();
    let mut best_size = 0u64;
    let mut best_url = String::new();

    let mut pos = 0;
    let ba = assets_json.as_bytes();
    while pos < ba.len() {
        if ba[pos] == b'{' {
            let mut br = 1u32;
            let mut end = pos + 1;
            while end < ba.len() && br > 0 {
                if ba[end] == b'{' {
                    br += 1;
                } else if ba[end] == b'}' {
                    br -= 1;
                }
                end += 1;
            }
            let obj_str = std::str::from_utf8(&ba[pos..end]).ok()?;

            if let Some(asset_name) = extract_json_str(obj_str, "name") {
                let is_appimage =
                    asset_name.ends_with(".AppImage") || asset_name.ends_with(".appimage");
                let matches = if let Some(pat) = pattern {
                    if is_appimage && pat.ends_with(".zsync") {
                        fnmatch(&pat[..pat.len() - 6], asset_name)
                    } else {
                        fnmatch(pat, asset_name)
                    }
                } else {
                    let asset_lower = asset_name.to_lowercase();
                    is_appimage && asset_lower.contains(&name_lower)
                };
                if matches {
                    let dk = "\"browser_download_url\":\"";
                    let url_val = obj_str
                        .find(dk)
                        .and_then(|ds| {
                            let d_rest = &obj_str[ds + dk.len()..];
                            let d_end = d_rest.find('"')?;
                            Some(&d_rest[..d_end])
                        })
                        .unwrap_or("");
                    let sk = "\"size\":";
                    let size_val = obj_str
                        .find(sk)
                        .and_then(|ss| {
                            let s_rest = &obj_str[ss + sk.len()..];
                            let s_end = s_rest.find(|c: char| !c.is_ascii_digit())?;
                            s_rest[..s_end].parse::<u64>().ok()
                        })
                        .unwrap_or(0);
                    if size_val > best_size {
                        best_size = size_val;
                        best_url = url_val.to_string();
                    }
                }
            }
            pos = end;
        } else {
            pos += 1;
        }
    }

    if best_size > 0 && !best_url.is_empty() {
        Some((tag, best_size, best_url))
    } else {
        None
    }
}
