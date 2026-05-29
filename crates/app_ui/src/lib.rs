use crate::state::{Action, AppState, SortMode, Store};
use crate::theme::*;
use crate::widgets::*;
use domain::{PackageDetails, PackageSummary, Source};
use repose_core::*;
use repose_ui::{
    lazy::{LazyColumn, LazyColumnState},
    scroll::{ScrollArea, remember_scroll_state},
    *,
};
use std::rc::Rc;

pub mod state;
pub mod theme;
pub mod widgets;

const PANE_HEIGHT_DP: f32 = 520.0;
const LOG_HEIGHT_DP: f32 = 180.0;

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
            chip("Installed", s.filter_installed, {
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

fn pkg_row(store: Rc<Store>, pkg: PackageSummary, selected: bool, upgrades_mode: bool) -> View {
    let is_aur = pkg.id.source == Source::Aur;

    let (bg, border) = if selected {
        (SEL_BG, SEL_BORDER)
    } else if is_aur {
        (AUR_BG, AUR_BORDER)
    } else {
        (CARD_BG, CARD_BORDER)
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
        Column(Modifier::new().flex_grow(1.0)).child((
            Row(Modifier::new()).child((
                Text(pkg.id.name.clone())
                    .size(FONT_LG)
                    .color(Color::from_hex(TEXT_PRIMARY)),
                Space(Modifier::new().width(8.0)),
                source_badge(is_aur),
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
            .height(PANE_HEIGHT_DP)
            .clip_rounded(R_SM)
            .padding(4.0),
        move |pkg: PackageSummary, _| {
            let is_sel = selected.as_ref().is_some_and(|id| *id == pkg.id);
            pkg_row(store.clone(), pkg, is_sel, upgrades_mode)
        },
    )
}

fn details_pane(store: Rc<Store>, s: &AppState) -> View {
    let Some(sel_id) = &s.selected else {
        return empty_state("Package details", "Select a package on the left.");
    };

    let summary = match s.results.iter().find(|p| &p.id == sel_id) {
        Some(p) => p.clone(),
        None => return empty_state("Package details", "Selection not in current results."),
    };

    let is_aur = summary.id.source == Source::Aur;

    let header = Column(Modifier::new().fill_max_width()).child((
        Row(Modifier::new()).child((
            Text(summary.id.name.clone())
                .size(FONT_XL)
                .color(Color::from_hex(TEXT_PRIMARY)),
            Space(Modifier::new().width(8.0)),
            source_badge(is_aur),
            Space(Modifier::new().width(4.0)),
            if summary.installed {
                installed_badge()
            } else {
                Box(Modifier::new())
            },
        )),
        Space(Modifier::new().height(4.0)),
        Text(format!("v{}", summary.version))
            .size(FONT_SM)
            .color(Color::from_hex(TEXT_DIMMED)),
        Space(Modifier::new().height(10.0)),
        Text(summary.description.clone())
            .size(FONT_BASE)
            .color(Color::from_hex(TEXT_SECONDARY))
            .max_lines(6)
            .overflow_ellipsize(),
    ));

    let actions = Row(Modifier::new().padding_values(PaddingValues {
        left: 0.0,
        right: 0.0,
        top: 12.0,
        bottom: 4.0,
    }))
    .child((
        pkg_action(&store, &summary, s.in_upgrades_view),
        Space(Modifier::new().width(8.0)),
        secondary_button("Clear", {
            let store = store.clone();
            move || store.dispatch(Action::ClearSelection)
        }),
    ));

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
        Modifier::new()
            .fill_max_width()
            .height(PANE_HEIGHT_DP)
            .then(card_mod()),
        scroll,
        Column(Modifier::new().fill_max_width().padding(16.0)).child((
            header,
            divider(),
            actions,
            Space(Modifier::new().height(8.0)),
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

    Row(Modifier::new()
        .fill_max_width()
        .padding(10.0)
        .margin_vertical(4.0)
        .background(Color::from_hex(CARD_BG))
        .border(1.0, Color::from_hex(CARD_BORDER), R_MD)
        .clip_rounded(R_MD))
    .child((
        Text("●")
            .size(8.0)
            .color(Color::from_hex(STATUS_DOT))
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

pub fn root_view(store: Rc<Store>) -> View {
    let s = store.state.get();

    Surface(
        Modifier::new()
            .fill_max_size()
            .background_brush(v_gradient(BG_START, BG_END)),
        Column(Modifier::new().fill_max_size().padding(16.0)).child((
            top_bar(&store, &s),
            divider(),
            search_section(&store, &s),
            Space(Modifier::new().height(8.0)),
            Row(Modifier::new().fill_max_width().flex_grow(1.0).flex_basis(0.0)).child((
                Box(Modifier::new().weight(7.0).padding(4.0)).child(results_list(&store, &s)),
                Space(Modifier::new().width(8.0)),
                Box(Modifier::new().weight(3.0).padding(4.0))
                    .child(details_pane(store.clone(), &s)),
            )),
            status_bar(&store, &s),
            log_panel(&s),
        )),
    )
}
