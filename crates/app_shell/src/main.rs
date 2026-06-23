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
use backend_alpm::AlpmBackend;
use backend_aur::AurBackend;
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

    let repo_backend: Arc<dyn PackageBackend> = Arc::new(AlpmBackend::new());
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
            // Callback-style watcher; coalesce by just sending a signal.
            const LOCAL_DB: &str = "/var/lib/pacman/local";
            // Debounce so we emit at most once per cooldown.
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

                    // Only react to meaningful changes.
                    let is_meaningful_kind = matches!(
                        ev.kind,
                        EventKind::Create(CreateKind::Folder)
                            | EventKind::Remove(RemoveKind::Folder)
                            | EventKind::Modify(ModifyKind::Name(_))
                            | EventKind::Create(CreateKind::File)
                            | EventKind::Remove(RemoveKind::File)
                    );
                    if !is_meaningful_kind {
                        return; // ignore Access/Metadata/etc.
                    }

                    // Only if paths are under the local DB and relevant:
                    let relevant = ev.paths.iter().any(|p| {
                        if !p.starts_with(LOCAL_DB) {
                            return false;
                        }
                        match ev.kind {
                            EventKind::Create(CreateKind::Folder)
                            | EventKind::Remove(RemoveKind::Folder) => {
                                // Only act on directories directly under .../local (pkg-version dirs)
                                p.parent()
                                    .map(|pp| pp == Path::new(LOCAL_DB))
                                    .unwrap_or(false)
                            }
                            EventKind::Modify(ModifyKind::Name(_)) => true, // rename within tree
                            EventKind::Create(CreateKind::File)
                            | EventKind::Remove(RemoveKind::File) => {
                                // Strict, only desc file
                                p.file_name().is_some_and(|f| f == "desc")
                            }
                            _ => false,
                        }
                    });
                    if !relevant {
                        return;
                    }

                    // Debounce
                    let now = Instant::now();
                    if now.duration_since(last) >= cooldown {
                        last = now;
                        let _ = tx_watch.send(());
                    }
                })
                .expect("watcher failed — check inotify limits (/proc/sys/fs/inotify/max_user_watches)");

            // Watch the local DB (recursive to see renames and file-level events as needed)
            if let Err(e) = watcher.watch(Path::new(LOCAL_DB), RecursiveMode::Recursive) {
                error!("package DB watcher failed to start: {e}");
                return;
            }
            // Keep thread alive indefinitely.
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
