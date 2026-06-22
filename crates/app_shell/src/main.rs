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
    theme::setup_theme,
};
use backend_aur::AurBackend;
use backend_pacman::PacmanCli;
use domain::{Executor, PackageBackend};
use log::error;
use repose_platform::run_desktop_app;
use repose_ui::overlay::{OverlayHandle, SnackbarController};

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let (tx_jobs, rx_jobs) = chan::unbounded();
    let (tx_prog, rx_prog) = chan::unbounded();
    let (tx_evt, rx_evt) = chan::unbounded();
    let (tx_watch, rx_watch) = chan::unbounded::<()>();

    let repo_backend: Arc<dyn PackageBackend> = Arc::new(PacmanCli::new());
    let aur_backend: Arc<dyn PackageBackend> = Arc::new(AurBackend::new());
    Executor::new(
        repo_backend,
        aur_backend,
        tx_prog.clone(),
        tx_evt.clone(),
        rx_jobs,
    )
    .run();

    let overlay = OverlayHandle::new();
    let snackbar = SnackbarController::new(overlay.clone());

    let store = Rc::new(Store::new(tx_jobs, Some(snackbar.clone())));

    {
        spawn(move || {
            const LOCAL_DB: &str = "/var/lib/pacman/local";
            let cooldown = Duration::from_millis(1200);
            let mut last = Instant::now() - cooldown;

            let mut watcher =
                notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
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
                })
                .expect("watcher failed");

            if let Err(e) = watcher.watch(Path::new(LOCAL_DB), RecursiveMode::Recursive) {
                error!("package DB watcher failed to start: {e}");
                return;
            }
            std::thread::park();
        });
    }

    setup_theme();

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
