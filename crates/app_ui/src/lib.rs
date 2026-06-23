use crate::state::{Action, AppState, Route, SortMode, Store};
use crate::theme::*;
use crate::widgets::*;
use domain::{PackageDetails, PackageSummary, Source};
use repose_core::*;
use repose_material::material3::{
    LinearProgressIndicator, LinearProgressIndicatorConfig, Surface, SurfaceConfig,
};
use repose_navigation::{NavDisplay, Navigator, NavTransition, EntryScope, renderer, remember_back_stack};
use repose_ui::{
    lazy::{LazyColumn, LazyColumnState},
    scroll::{ScrollArea, remember_scroll_state},
    *,
};
use std::rc::Rc;

pub mod state;
pub mod theme;
pub mod widgets;

const LOG_HEIGHT_DP: f32 = 180.0;
const AVATAR_SIZE: f32 = 36.0;
const AVATAR_SIZE_LG: f32 = 64.0;

fn top_bar(store: &Rc<Store>, s: &AppState) -> View {
    let title = if s.in_upgrades_view {
        "Upgrades"
    } else {
        "Soredowe"
    };

    Row(Modifier::new()
        .fill_max_width()
        .padding_values(PaddingValues {
            left: 16.0,
            right: 16.0,
            top: 12.0,
            bottom: 12.0,
        }))
    .child((
        Text(title)
            .size(FONT_2XL)
            .color(Color::from_hex(TEXT_PRIMARY))
            .modifier(Modifier::new().align_self_center()),
        Spacer(),
        if s.in_upgrades_view && !s.results.is_empty() {
            success_button("Upgrade all", {
                let store = store.clone();
                move || store.dispatch(Action::UpgradeAll)
            })
        } else {
            Box(Modifier::new())
        },
        Space(Modifier::new().width(8.0)),
        secondary_button("Refresh", {
            let store = store.clone();
            move || store.dispatch(Action::Refresh)
        }),
        Space(Modifier::new().width(8.0)),
        primary_button("Upgrades", {
            let store = store.clone();
            move || store.dispatch(Action::Upgrades)
        }),
    ))
}

fn search_section(store: &Rc<Store>, s: &AppState) -> View {
    Column(
        Modifier::new()
            .fill_max_width()
            .padding_values(PaddingValues {
                left: 16.0,
                right: 16.0,
                top: 8.0,
                bottom: 8.0,
            }),
    )
    .child((
        Row(Modifier::new().fill_max_width()).child((
            TextField(
                "Search packages…",
                s.query.clone(),
                Modifier::new()
                    .flex_grow(1.0)
                    .max_width(500.0)
                    .height(40.0)
                    .padding_values(PaddingValues {
                        left: 12.0,
                        right: 12.0,
                        top: 0.0,
                        bottom: 0.0,
                    })
                    .background(Color::from_hex(CARD_BG))
                    .border(1.0, Color::from_hex(CARD_BORDER), R_MD)
                    .clip_rounded(R_MD)
                    .semantics(Semantics {
                        role: Role::TextField,
                        label: Some("Search field".into()),
                        focused: false,
                        enabled: true,
                    }),
                Some({
                    let store = store.clone();
                    move |text: String| store.dispatch(Action::SetQuery(text))
                }),
                Some({
                    let store = store.clone();
                    move |text: String| {
                        store.dispatch(Action::SetQuery(text));
                        store.dispatch(Action::Search);
                    }
                }),
            ),
            Space(Modifier::new().width(8.0)),
            primary_button("Search", {
                let store = store.clone();
                move || store.dispatch(Action::Search)
            }),
        )),
        Space(Modifier::new().height(8.0)),
        Row(Modifier::new().fill_max_width()).child(vec![
            chip("Repo", s.filter_repo, {
                let store = store.clone();
                move || store.dispatch(Action::ToggleFilterRepo)
            }),
            Space(Modifier::new().width(6.0)),
            chip("AUR", s.filter_aur, {
                let store = store.clone();
                move || store.dispatch(Action::ToggleFilterAur)
            }),
            Space(Modifier::new().width(6.0)),
            chip("Flatpak", s.filter_flatpak, {
                let store = store.clone();
                move || store.dispatch(Action::ToggleFilterFlatpak)
            }),
            Space(Modifier::new().width(6.0)),
            chip("AppImage", s.filter_appimage, {
                let store = store.clone();
                move || store.dispatch(Action::ToggleFilterAppImage)
            }),
            Space(Modifier::new().width(6.0)),
            chip("Installed only", s.filter_installed, {
                let store = store.clone();
                move || store.dispatch(Action::ToggleFilterInstalled)
            }),
            Spacer(),
            Text("Sort:")
                .size(FONT_SM)
                .color(Color::from_hex(TEXT_DIMMED))
                .modifier(Modifier::new().align_self_center()),
            Space(Modifier::new().width(6.0)),
            chip("Popular", s.sort == SortMode::Popularity, {
                let store = store.clone();
                move || store.dispatch(Action::SetSort(SortMode::Popularity))
            }),
            Space(Modifier::new().width(4.0)),
            chip("A-Z", s.sort == SortMode::NameAsc, {
                let store = store.clone();
                move || store.dispatch(Action::SetSort(SortMode::NameAsc))
            }),
            Space(Modifier::new().width(4.0)),
            chip("Z-A", s.sort == SortMode::NameDesc, {
                let store = store.clone();
                move || store.dispatch(Action::SetSort(SortMode::NameDesc))
            }),
        ]),
    ))
}

fn pkg_action(store: &Rc<Store>, pkg: &PackageSummary, upgrades_mode: bool) -> View {
    let id = pkg.id.clone();
    if upgrades_mode {
        let s = store.clone();
        success_button("Upgrade", move || s.dispatch(Action::Upgrade(id.clone())))
    } else if pkg.installed {
        let s = store.clone();
        danger_button("Remove", move || s.dispatch(Action::Remove(id.clone())))
    } else {
        let s = store.clone();
        success_button("Install", move || s.dispatch(Action::Install(id.clone())))
    }
}

fn pkg_row(store: Rc<Store>, pkg: PackageSummary, _selected: bool, upgrades_mode: bool) -> View {
    let (bg, border) = match pkg.id.source {
        Source::Aur => (AUR_BG, AUR_BORDER),
        Source::Flatpak => (FLATPAK_BG, FLATPAK_BORDER),
        Source::AppImage => (APPIMAGE_BG, APPIMAGE_BORDER),
        Source::Repo => (CARD_BG, CARD_BORDER),
    };

    Row(Modifier::new()
        .fill_max_width()
        .padding(12.0)
        .margin_vertical(3.0)
        .background(Color::from_hex(bg))
        .border(1.0, Color::from_hex(border), R_MD)
        .clip_rounded(R_MD)
        .clickable()
        .on_pointer_down({
            let store = store.clone();
            let id = pkg.id.clone();
            move |_| store.dispatch(Action::Select(id.clone()))
        }))
    .child((
        pkg_avatar(&pkg.id.name, AVATAR_SIZE),
        Space(Modifier::new().width(12.0)),
        Column(Modifier::new().flex_grow(1.0)).child((
        Row(Modifier::new().align_items(AlignItems::Center)).child((
            Text(pkg.id.name.clone())
                .size(FONT_LG)
                .color(Color::from_hex(TEXT_PRIMARY)),
            Space(Modifier::new().width(8.0)),
            source_badge(&pkg),
            Space(Modifier::new().width(4.0)),
            if pkg.installed {
                installed_badge()
            } else {
                Box(Modifier::new())
            },
        )),
            Space(Modifier::new().height(4.0)),
            Text(pkg.description.clone())
                .size(FONT_SM)
                .color(Color::from_hex(TEXT_MUTED))
                .max_lines(1)
                .overflow_ellipsize()
                .modifier(Modifier::new().max_width(480.0)),
        )),
        Column(Modifier::new().align_self_center()).child((
            Text(pkg.version.clone())
                .size(FONT_XS)
                .color(Color::from_hex(TEXT_DIMMED))
                .modifier(
                    Modifier::new()
                        .align_self_center()
                        .padding_values(PaddingValues {
                            left: 0.0,
                            right: 0.0,
                            top: 0.0,
                            bottom: 4.0,
                        }),
                ),
            pkg_action(&store, &pkg, upgrades_mode),
        )),
    ))
}

fn results_list(store: &Rc<Store>, s: &AppState) -> View {
    if s.results.is_empty() {
        return empty_state(
            "No results",
            if s.query.trim().is_empty() {
                "Search for a package or check upgrades."
            } else {
                "No packages matched your query / filters."
            },
        );
    }

    let store = store.clone();
    let selected = s.selected.clone();
    let upgrades_mode = s.in_upgrades_view;
    let results = s.results.clone();

    LazyColumn(
        results,
        76.0,
        remember_with_key("pkg_scroll", LazyColumnState::new),
        Modifier::new()
            .fill_max_width()
            .flex_grow(1.0)
            .clip_rounded(R_SM)
            .padding(4.0),
        |pkg: &PackageSummary| {
            let mut h: u64 = 0;
            for b in pkg.id.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u64);
            }
            h
        },
        None::<repose_core::animation::AnimationSpec>,
        move |pkg: PackageSummary, _| {
            let is_sel = selected.as_ref().is_some_and(|id| *id == pkg.id);
            pkg_row(store.clone(), pkg, is_sel, upgrades_mode)
        },
    )
}

fn detail_overlay(store: Rc<Store>, s: &AppState) -> View {
    let Some(sel_id) = &s.selected else {
        return Box(Modifier::new());
    };

    let summary = match s.results.iter().find(|p| &p.id == sel_id) {
        Some(p) => p.clone(),
        None => return Box(Modifier::new()),
    };

    let detail_section: View = if let Some(det) = &s.detail {
        details_body(det)
    } else {
        Text("Loading details…")
            .size(FONT_SM)
            .color(Color::from_hex(TEXT_DIMMED))
            .modifier(Modifier::new().padding(8.0))
    };

    let scroll = remember_scroll_state(format!(
        "detail:{}:{:?}",
        summary.id.name, summary.id.source
    ));

    ScrollArea(
        Modifier::new().fill_max_size().padding(24.0),
        scroll,
        Column(Modifier::new().fill_max_size().max_width(780.0).align_self_center()).child((
            Row(Modifier::new().fill_max_width().align_items(AlignItems::Center)).child((
                secondary_button("Back", {
                    let store = store.clone();
                    move || store.dispatch(Action::ClearSelection)
                }),
                Spacer(),
                pkg_action(&store, &summary, s.in_upgrades_view),
            )),
            Space(Modifier::new().height(20.0)),
            Row(Modifier::new().fill_max_width()).child((
                pkg_avatar(&summary.id.name, AVATAR_SIZE_LG),
                Space(Modifier::new().width(20.0)),
                Column(Modifier::new().flex_grow(1.0)).child((
                    Row(Modifier::new().align_items(AlignItems::Center)).child((
                        Text(summary.id.name.clone())
                            .size(28.0)
                            .color(Color::from_hex(TEXT_PRIMARY)),
                        Space(Modifier::new().width(12.0)),
                        source_badge(&summary),
                        if summary.installed {
                            installed_badge()
                        } else {
                            Box(Modifier::new())
                        },
                    )),
                    Space(Modifier::new().height(6.0)),
                    Text(summary.description.clone())
                        .size(FONT_BASE)
                        .color(Color::from_hex(TEXT_SECONDARY))
                        .max_lines(6)
                        .overflow_ellipsize(),
                    Space(Modifier::new().height(4.0)),
                    Text(format!("v{}", summary.version))
                        .size(FONT_SM)
                        .color(Color::from_hex(TEXT_DIMMED)),
                )),
            )),
            Space(Modifier::new().height(16.0)),
            divider(),
            Space(Modifier::new().height(12.0)),
            detail_section,
        )),
    )
}

fn details_body(det: &PackageDetails) -> View {
    Column(Modifier::new().fill_max_width()).child((
        detail_row("Homepage", det.homepage.as_deref().unwrap_or("")),
        detail_row("Maintainer", det.maintainer.as_deref().unwrap_or("unknown")),
        detail_row(
            "Install size",
            &det.size_install
                .map(|b| format_bytes(b))
                .unwrap_or_default(),
        ),
        detail_row(
            "Download",
            &det.size_download
                .map(|b| format_bytes(b))
                .unwrap_or_default(),
        ),
        tag_list("Dependencies", &det.depends),
        tag_list("Optional deps", &det.opt_depends),
    ))
}

fn status_bar(store: &Rc<Store>, s: &AppState) -> View {
    let last = s.progress_log.lines().last().unwrap_or("Ready");
    let stage_label = s.active_stage.map(|st| format!("{:?}", st));

    let indicator = if let Some(stage) = &stage_label {
        Row(Modifier::new().fill_max_width().align_items(AlignItems::Center)).child((
            Text(stage.as_str())
                .size(FONT_XS)
                .color(Color::from_hex(INDIGO))
                .modifier(Modifier::new().padding(4.0).width(100.0)),
            if let Some(pct) = s.progress_pct {
                LinearProgressIndicator(
                    Some(pct),
                    LinearProgressIndicatorConfig {
                        color: Color::from_hex(BLUE_BORDER),
                        track_color: Color::from_hex(CARD_BORDER),
                        ..Default::default()
                    },
                )
            } else {
                LinearProgressIndicator(
                    None,
                    LinearProgressIndicatorConfig {
                        color: Color::from_hex(INDIGO),
                        track_color: Color::from_hex(CARD_BORDER),
                        ..Default::default()
                    },
                )
            },
            Space(Modifier::new().width(4.0)),
            if let Some(pct) = s.progress_pct {
                Text(format!("{:.0}%", pct * 100.0))
                    .size(FONT_XS)
                    .color(Color::from_hex(TEXT_DIMMED))
                    .modifier(Modifier::new().width(40.0))
            } else {
                Box(Modifier::new().width(40.0))
            },
        ))
    } else {
        Box(Modifier::new().height(6.0))
    };

    Column(Modifier::new().fill_max_width().margin_vertical(4.0)).child((
        indicator,
        Row(Modifier::new()
            .fill_max_width()
            .padding(10.0)
            .background(Color::from_hex(CARD_BG))
            .border(1.0, Color::from_hex(CARD_BORDER), R_MD)
            .clip_rounded(R_MD))
        .child((
            Text("●")
                .size(8.0)
                .color(Color::from_hex(if s.active_stage.is_some() { INDIGO } else { STATUS_DOT }))
                .modifier(Modifier::new().align_self_center().padding(4.0)),
            Text(last.to_string())
                .size(FONT_SM)
                .color(Color::from_hex(TEXT_MUTED))
                .modifier(Modifier::new().flex_grow(1.0).align_self_center()),
            secondary_button(
                if s.log_expanded {
                    "Hide log"
                } else {
                    "Show log"
                },
                {
                    let store = store.clone();
                    move || store.dispatch(Action::ToggleLog)
                },
            ),
        )),
    ))
}

fn log_panel(s: &AppState) -> View {
    if !s.log_expanded {
        return Box(Modifier::new());
    }

    let scroll = remember_scroll_state("log_panel");

    ScrollArea(
        Modifier::new()
            .fill_max_width()
            .height(LOG_HEIGHT_DP)
            .padding(12.0)
            .margin_vertical(4.0)
            .background(Color::from_hex(CARD_BG))
            .border(1.0, Color::from_hex(CARD_BORDER), R_MD)
            .clip_rounded(R_MD),
        scroll,
        Column(Modifier::new().fill_max_width()).child(
            Text(s.progress_log.clone())
                .size(12.0)
                .color(Color::from_hex(LOG_TEXT))
                .modifier(Modifier::new().fill_max_width()),
        ),
    )
}

fn home_view(store: Rc<Store>) -> View {
    let s = store.state.get();
    Column(Modifier::new().fill_max_size().padding(16.0)).child((
        top_bar(&store, &s),
        divider(),
        search_section(&store, &s),
        Space(Modifier::new().height(8.0)),
        results_list(&store, &s),
        status_bar(&store, &s),
        log_panel(&s),
    ))
}

pub fn root_view(store: Rc<Store>) -> View {
    let stack = remember_back_stack(Route::Home);
    *store.navigator.borrow_mut() = Some(Navigator { stack: (*stack).clone() });

    Surface(
        SurfaceConfig {
            modifier: Modifier::new()
                .fill_max_size()
                .background_brush(v_gradient(BG_START, BG_END)),
            ..Default::default()
        },
        || {
            NavDisplay(
                stack.clone(),
                renderer(move |scope: &EntryScope<Route>| match scope.key() {
                    Route::Home => home_view(store.clone()),
                    Route::Detail(_) => {
                        let s = store.state.get();
                        detail_overlay(store.clone(), &s)
                    }
                }),
                None,
                NavTransition::default(),
            )
        },
    )
}
