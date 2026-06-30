use crossbeam_channel as chan;
use log::error;
use repose_core::Modifier;
use std::{rc::Rc, sync::Arc};

use app_ui::{
    root_view,
    state::{Action, Store},
};
use domain::config::Settings;
use domain::{Executor, PackageBackend};
use repose_platform::run_desktop_app;
use repose_ui::overlay::{OverlayHandle, SnackbarController};

#[cfg(feature = "backend-alpm")]
use backend_alpm::AlpmBackend;
#[cfg(feature = "backend-appimage")]
use backend_appimage::AppImageBackend;
#[cfg(feature = "backend-aur")]
use backend_aur::AurBackend;
#[cfg(feature = "backend-flatpak")]
use backend_flatpak::FlatpakBackend;
#[cfg(feature = "backend-packagekit")]
use backend_packagekit::PackageKitBackend;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let settings = Settings::load();

    let (tx_jobs, rx_jobs) = chan::unbounded();
    let (tx_prog, rx_prog) = chan::unbounded();
    let (tx_evt, rx_evt) = chan::unbounded();

    #[allow(unused_mut)]
    let mut backends: Vec<(&'static str, Arc<dyn PackageBackend>)> = Vec::new();

    #[cfg(feature = "backend-alpm")]
    if settings.enable_repo {
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
    if settings.enable_aur {
        let aur: Arc<dyn PackageBackend> = Arc::new(AurBackend::new());
        backends.push((aur.name(), aur));
    }

    #[cfg(feature = "backend-flatpak")]
    if settings.enable_flatpak {
        match FlatpakBackend::new(true) {
            Ok(fp) => {
                let flatpak: Arc<dyn PackageBackend> = Arc::new(fp);
                backends.push((flatpak.name(), flatpak));
            }
            Err(e) => error!("Flatpak backend unavailable: {e}"),
        }
    }

    #[cfg(feature = "backend-appimage")]
    if settings.enable_appimage {
        let appimage: Arc<dyn PackageBackend> = Arc::new(AppImageBackend::new());
        backends.push((appimage.name(), appimage));
    }

    Executor::new(backends, tx_prog.clone(), tx_evt.clone(), rx_jobs).run();

    let overlay = OverlayHandle::new();
    let snackbar = SnackbarController::new(overlay.clone());

    let store = Rc::new(Store::new(tx_jobs, Some(snackbar), settings));
    // store.dispatch(Action::Refresh);  HACK: enable it once you figure out how octopi can update repos without polkit (is done via local repo updation, but a better soln might also be possible?)

    run_desktop_app(move |_sched, _ctx| {
        while let Ok(p) = rx_prog.try_recv() {
            store.dispatch(Action::Progress(p));
        }
        while let Ok(e) = rx_evt.try_recv() {
            store.dispatch(Action::Event(e));
        }
        overlay.host(Modifier::new().fill_max_size(), root_view(store.clone()))
    })
}
