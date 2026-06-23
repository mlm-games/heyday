use domain::*;
use std::{
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
};

pub struct AppImageBackend;

fn appimage_dirs() -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    vec![
        format!("{home}/Applications"),
        format!("{home}/.local/bin"),
        format!("{home}/Applications/AppImage"),
    ]
}

fn find_appimage(name: &str) -> Option<String> {
    for dir in appimage_dirs() {
        let path = format!("{dir}/{name}.AppImage");
        if Path::new(&path).exists() {
            return Some(path);
        }
    }
    None
}

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
        let path = find_appimage(&id.name).ok_or_else(|| {
            Error::AppImage(format!("{} not found", id.name))
        })?;
        let meta = std::fs::metadata(&path).map_err(|e| Error::AppImage(e.to_string()))?;
        let size = meta.len();

        Ok(PackageDetails {
            summary: PackageSummary {
                id: id.clone(),
                version: String::new(),
                description: String::new(),
                installed: true,
                popular: None,
                last_updated: None,
            },
            depends: vec![],
            opt_depends: vec![],
            homepage: None,
            maintainer: None,
            size_install: Some(size),
            size_download: None,
        })
    }

    fn installed(&self, _cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        let mut results = Vec::new();
        for dir in appimage_dirs() {
            let dir_path = Path::new(&dir);
            if !dir_path.is_dir() {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(dir_path) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("AppImage") {
                    continue;
                }
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                if name.is_empty() {
                    continue;
                }
                let meta = match std::fs::metadata(&path) {
                    Ok(m) => m,
                    _ => continue,
                };
                results.push(PackageSummary {
                    id: PackageId {
                        name,
                        source: Source::AppImage,
                        repo: None,
                    },
                    version: String::new(),
                    description: String::new(),
                    installed: true,
                    popular: None,
                    last_updated: meta.modified().ok(),
                });
            }
        }
        Ok(results)
    }

    fn updates(&self, _cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        Ok(vec![])
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

        let home =
            std::env::var("HOME").map_err(|_| Error::AppImage("$HOME not set".into()))?;
        let dir = format!("{home}/Applications");
        std::fs::create_dir_all(&dir).map_err(|e| Error::AppImage(e.to_string()))?;
        let dest = format!("{dir}/{}.AppImage", id.name);
        let tmp = format!("{dir}/.{}.AppImage.part", id.name);

        let agent = ureq::Agent::new_with_defaults();
        let resp = agent
            .get(url)
            .call()
            .map_err(|e: ureq::Error| Error::AppImage(format!("download failed: {e}")))?;

        let total: u64 = resp.body().content_length().unwrap_or(0);

        let mut reader = resp.into_body().into_reader();
        let mut file: std::fs::File =
            std::fs::File::create(&tmp).map_err(|e: std::io::Error| Error::AppImage(e.to_string()))?;
        let mut buf = [0u8; 65536];
        let mut downloaded: u64 = 0;

        loop {
            if cancel.is_cancelled() {
                let _ = std::fs::remove_file(&tmp);
                return Err(Error::Cancelled);
            }
            let n: usize = reader
                .read(&mut buf)
                .map_err(|e: std::io::Error| Error::AppImage(e.to_string()))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])
                .map_err(|e: std::io::Error| Error::AppImage(e.to_string()))?;
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
        std::fs::rename(&tmp, &dest).map_err(|e| Error::AppImage(e.to_string()))?;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| Error::AppImage(e.to_string()))?;

        Ok(())
    }

    fn remove(&self, id: &PackageId, _sink: &ProgressSink, _cancel: &CancelToken) -> Result<()> {
        let path =
            find_appimage(&id.name).ok_or_else(|| Error::AppImage(format!("{} not found", id.name)))?;
        std::fs::remove_file(&path).map_err(|e| Error::AppImage(e.to_string()))?;
        Ok(())
    }

    fn upgrade(&self, id: &PackageId, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        self.remove(id, sink, cancel)?;
        self.install(id, sink, cancel)
    }

    fn upgrade_all(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let installed = self.installed(cancel)?;
        for pkg in &installed {
            // Only upgrade packages that have a URL stored
            if pkg.id.repo.is_some() {
                self.upgrade(&pkg.id, sink, cancel)?;
            }
        }
        Ok(())
    }
}
