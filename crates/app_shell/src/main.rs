use crossbeam_channel as chan;
use notify::{
    EventKind, RecursiveMode, Watcher,
    event::{CreateKind, ModifyKind, RemoveKind},
};
use repose_core::Modifier;
use std::{
    path::Path,
    rc::Rc,
    sync::Arc,
    thread::spawn,
    time::{Duration, Instant},
};

use app_ui::{
    root_view,
    state::{Action, Store},
};
use domain::{Executor, PackageBackend};
use log::error;
use repose_platform::run_desktop_app;
use repose_ui::overlay::{OverlayHandle, SnackbarController};

#[cfg(feature = "backend-alpm")]
use backend_alpm::AlpmBackend;
#[cfg(feature = "backend-packagekit")]
use backend_packagekit::PackageKitBackend;
#[cfg(feature = "backend-aur")]
use backend_aur::AurBackend;
#[cfg(feature = "backend-flatpak")]
use backend_flatpak::FlatpakBackend;
#[cfg(feature = "backend-appimage")]
use backend_appimage::AppImageBackend;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let (tx_jobs, rx_jobs) = chan::unbounded();
    let (tx_prog, rx_prog) = chan::unbounded();
    let (tx_evt, rx_evt) = chan::unbounded();
    let (tx_watch, rx_watch) = chan::unbounded::<()>();

    #[allow(unused_mut)]
    let mut backends: Vec<(&'static str, Arc<dyn PackageBackend>)> = Vec::new();

    #[cfg(feature = "backend-alpm")]
    {
        let repo: Arc<dyn PackageBackend> = Arc::new(AlpmBackend::new());
        backends.push((repo.name(), repo));
    }

    #[cfg(feature = "backend-packagekit")]
    match PackageKitBackend::new() {
        Ok(pk) => {
            let pk: Arc<dyn PackageBackend> = Arc::new(pk);
            backends.push((pk.name(), pk));
        }
        Err(e) => error!("PackageKit backend unavailable: {e}"),
    }

    #[cfg(feature = "backend-aur")]
    {
        let aur: Arc<dyn PackageBackend> = Arc::new(AurBackend::new());
        backends.push((aur.name(), aur));
    }

    #[cfg(feature = "backend-flatpak")]
    {
        let flatpak: Arc<dyn PackageBackend> = Arc::new(FlatpakBackend::new(true));
        backends.push((flatpak.name(), flatpak));
    }

    #[cfg(feature = "backend-appimage")]
    {
        let appimage: Arc<dyn PackageBackend> = Arc::new(AppImageBackend::new());
        backends.push((appimage.name(), appimage));
    }

    Executor::new(backends, tx_prog.clone(), tx_evt.clone(), rx_jobs).run();

    let overlay = OverlayHandle::new();
    let snackbar = SnackbarController::new(overlay.clone());

    let store = Rc::new(Store::new(tx_jobs, Some(snackbar)));

    {
        spawn(move || {
            const LOCAL_DB: &str = "/var/lib/pacman/local";
            let cooldown = Duration::from_millis(1200);
            let mut last = Instant::now() - cooldown;

            let mut watcher = notify::recommended_watcher(
                move |res: notify::Result<notify::Event>| {
                    let ev = match res {
                        Ok(e) => e,
                        Err(e) => {
                            error!("file watcher error: {e}");
                            return;
                        }
                    };

                    let is_meaningful_kind = matches!(
                        ev.kind,
                        EventKind::Create(CreateKind::Folder)
                            | EventKind::Remove(RemoveKind::Folder)
                            | EventKind::Modify(ModifyKind::Name(_))
                            | EventKind::Create(CreateKind::File)
                            | EventKind::Remove(RemoveKind::File)
                    );
                    if !is_meaningful_kind {
                        return;
                    }

                    let relevant = ev.paths.iter().any(|p| {
                        if !p.starts_with(LOCAL_DB) {
                            return false;
                        }
                        match ev.kind {
                            EventKind::Create(CreateKind::Folder)
                            | EventKind::Remove(RemoveKind::Folder) => {
                                p.parent()
                                    .map(|pp| pp == Path::new(LOCAL_DB))
                                    .unwrap_or(false)
                            }
                            EventKind::Modify(ModifyKind::Name(_)) => true,
                            EventKind::Create(CreateKind::File)
                            | EventKind::Remove(RemoveKind::File) => {
                                p.file_name().is_some_and(|f| f == "desc")
                            }
                            _ => false,
                        }
                    });
                    if !relevant {
                        return;
                    }

                    let now = Instant::now();
                    if now.duration_since(last) >= cooldown {
                        last = now;
                        let _ = tx_watch.send(());
                    }
                },
            )
            .expect(
                "watcher failed -- check inotify limits (/proc/sys/fs/inotify/max_user_watches)",
            );

            if let Err(e) = watcher.watch(Path::new(LOCAL_DB), RecursiveMode::Recursive) {
                error!("package DB watcher failed to start: {e}");
                return;
            }
            std::thread::park();
        });
    }

    run_desktop_app(move |_sched, _ctx| {
        while let Ok(p) = rx_prog.try_recv() {
            store.dispatch(Action::Progress(p));
        }
        while let Ok(e) = rx_evt.try_recv() {
            store.dispatch(Action::Event(e));
        }
        let mut saw = false;
        while rx_watch.try_recv().is_ok() {
            saw = true;
        }
        if saw {
            store.dispatch(Action::Event(domain::Event::SystemChanged));
        }
        overlay.host(Modifier::new().fill_max_size(), root_view(store.clone()))
    })
}
