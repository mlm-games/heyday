use crate::state::{Action, SortMode, Store};
use crate::theme::*;
use crate::widgets::*;
use domain::{PackageSummary, Source};
use repose_core::*;
use repose_ui::{
    lazy::{LazyColumn, LazyColumnState},
    *,
};
use std::rc::Rc;

pub mod state;
pub mod theme;
pub mod widgets;

fn pkg_action_button(store: &Rc<Store>, pkg: &PackageSummary, upgrades_mode: bool) -> View {
    let store = store.clone();
    let id = pkg.id.clone();
    if upgrades_mode {
        success_button("Upgrade", move || {
            store.dispatch(Action::Upgrade(id.clone()))
        })
    } else if pkg.installed {
        danger_button("Remove", move || store.dispatch(Action::Remove(id.clone())))
    } else {
        success_button("Install", move || {
            store.dispatch(Action::Install(id.clone()))
        })
    }
}

fn pkg_row(store: Rc<Store>, pkg: PackageSummary, selected: bool, upgrades_mode: bool) -> View {
    let is_aur = pkg.id.source == Source::Aur;

    let (bg_start, bg_end, border) = if selected {
        (SELECTED_START, SELECTED_END, SELECTED_BORDER)
    } else if is_aur {
        (AUR_START, AUR_END, AUR_BORDER)
    } else {
        (CARD_START, CARD_END, CARD_BORDER)
    };

    Row(Modifier::new()
        .padding(16.0)
        .margin_vertical(8.0)
        .background_brush(diag_gradient(bg_start, bg_end))
        .border(1.0, Color::from_hex(border), CORNER_LG)
        .clip_rounded(CORNER_LG)
        .clickable()
        .on_pointer_down({
            let store = store.clone();
            let id = pkg.id.clone();
            move |_| store.dispatch(Action::Select(id.clone()))
        }))
    .child((
        Column(Modifier::new().flex_grow(1.0)).child((
            // Name row
            Row(Modifier::new().padding_values(PaddingValues {
                left: 0.0,
                right: 0.0,
                top: 0.0,
                bottom: 8.0,
            }))
            .child((
                Text(pkg.id.name.clone())
                    .size(PKG_NAME_FONT)
                    .color(Color::from_hex(TEXT_PRIMARY))
                    .modifier(Modifier::new().padding(4.0)),
                Space(Modifier::new().width(8.0)),
                source_badge(is_aur),
                Space(Modifier::new().width(6.0)),
                if pkg.installed {
                    installed_badge()
                } else {
                    Box(Modifier::new())
                },
            )),
            // Description
            Text(pkg.description.clone())
                .size(SMALL_FONT)
                .color(Color::from_hex(TEXT_MUTED))
                .max_lines(2)
                .overflow_ellipsize()
                .modifier(Modifier::new().flex_grow(1.0).max_width(500.0)),
        )),
        Space(Modifier::new().width(16.0)),
        pkg_action_button(&store, &pkg, upgrades_mode),
    ))
}

fn details_card(store: Rc<Store>) -> View {
    let s = store.state.get();
    let results = s.results.clone();

    let Some(id) = &s.selected else {
        return empty_state("📦", "Select a package to see details", "");
    };

    let Some(pkg) = results.into_iter().find(|p| &p.id == id) else {
        return empty_state("", "No details available", "");
    };

    let is_aur = pkg.id.source == Source::Aur;

    Column(
        Modifier::new()
            .padding(24.0)
            .fill_max_width()
            .then(card_modifier()),
    )
    .child((
        // Header
        Row(Modifier::new().padding_values(PaddingValues {
            left: 0.0,
            right: 0.0,
            top: 0.0,
            bottom: 16.0,
        }))
        .child((
            Text(pkg.id.name.clone())
                .size(TITLE_FONT)
                .color(Color::from_hex(TEXT_PRIMARY)),
            Space(Modifier::new().width(12.0)),
            source_badge(is_aur),
            Space(Modifier::new().width(8.0)),
            if pkg.installed {
                installed_badge()
            } else {
                Box(Modifier::new())
            },
        )),
        divider(),
        // Description
        Text(pkg.description.clone())
            .max_lines(10)
            .overflow_clip()
            .size(BODY_FONT)
            .color(Color::from_hex(TEXT_SECONDARY))
            .modifier(Modifier::new().padding_values(PaddingValues {
                left: 0.0,
                right: 0.0,
                top: 0.0,
                bottom: 24.0,
            })),
        // Actions
        Row(Modifier::new()).child((
            pkg_action_button(&store, &pkg, s.in_upgrades_view),
            Space(Modifier::new().width(12.0)),
            secondary_button("Clear", {
                let store = store.clone();
                move || store.dispatch(Action::ClearSelection)
            }),
        )),
    ))
}

fn error_banner(store: &Rc<Store>, err: String) -> View {
    Row(Modifier::new()
        .padding(16.0)
        .margin_vertical(16.0)
        .background_brush(h_gradient(ERROR_BG_START, ERROR_BG_END))
        .border(1.0, Color::from_hex(ERROR_BORDER), CORNER_LG)
        .clip_rounded(CORNER_LG))
    .child((
        Text("⚠️").size(20.0).modifier(Modifier::new().padding(4.0)),
        Space(Modifier::new().width(12.0)),
        Text(err)
            .color(Color::from_hex(ERROR_TEXT))
            .size(BODY_FONT)
            .modifier(Modifier::new().flex_grow(1.0)),
        secondary_button("Dismiss", {
            let store = store.clone();
            move || store.dispatch(Action::ClearError)
        }),
    ))
}

fn header_bar(store: &Rc<Store>, s: &state::AppState) -> View {
    Row(Modifier::new().padding_values(PaddingValues {
        left: 8.0,
        right: 8.0,
        top: 8.0,
        bottom: 16.0,
    }))
    .child((
        Text(" ") // Empty space
            .size(HEADER_FONT)
            .color(Color::from_hex(TEXT_PRIMARY))
            .modifier(Modifier::new().padding(8.0)),
        Text(" ") // Empty space (to fill later)
            .size(BODY_FONT)
            .color(Color::from_hex(TEXT_DIMMED))
            .modifier(Modifier::new().padding(12.0).align_self_center()),
        Spacer(),
        if s.in_upgrades_view && !s.results.is_empty() {
            success_button("⬆ Upgrade All", {
                let store = store.clone();
                move || store.dispatch(Action::UpgradeAll)
            })
        } else {
            Box(Modifier::new())
        },
        Space(Modifier::new().width(12.0)),
        secondary_button("🔄 Refresh", {
            let store = store.clone();
            move || store.dispatch(Action::Refresh)
        }),
        Space(Modifier::new().width(12.0)),
        styled_button("📦 Upgrades", {
            let store = store.clone();
            move || store.dispatch(Action::Upgrades)
        }),
    ))
}

fn search_bar(store: &Rc<Store>, s: &state::AppState) -> View {
    Row(Modifier::new().padding_values(PaddingValues {
        left: 8.0,
        right: 8.0,
        top: 16.0,
        bottom: 16.0,
    }))
    .child(vec![
        TextField(
            "🔍 Search packages…",
            Modifier::new()
                .size(450.0, 44.0)
                .background_brush(v_gradient(CARD_START, CARD_END))
                .border(1.0, Color::from_hex(CARD_BORDER), CORNER_MD)
                .clip_rounded(CORNER_MD)
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
        Space(Modifier::new().width(12.0)),
        styled_button("Search", {
            let store = store.clone();
            move || store.dispatch(Action::Search)
        }),
        Space(Modifier::new().width(24.0)),
        // Source filters
        chip("Repo", s.filter_repo, {
            let store = store.clone();
            move || store.dispatch(Action::ToggleFilterRepo)
        }),
        Space(Modifier::new().width(8.0)),
        chip("AUR", s.filter_aur, {
            let store = store.clone();
            move || store.dispatch(Action::ToggleFilterAur)
        }),
        Space(Modifier::new().width(8.0)),
        chip("Installed", s.filter_installed, {
            let store = store.clone();
            move || store.dispatch(Action::ToggleFilterInstalled)
        }),
        Spacer(),
        // Sort
        Text("Sort:")
            .size(SMALL_FONT)
            .color(Color::from_hex(TEXT_DIMMED))
            .modifier(Modifier::new().padding(8.0).align_self_center()),
        Space(Modifier::new().width(8.0)),
        chip("A–Z", s.sort == SortMode::NameAsc, {
            let store = store.clone();
            move || store.dispatch(Action::SetSort(SortMode::NameAsc))
        }),
        Space(Modifier::new().width(6.0)),
        chip("Z–A", s.sort == SortMode::NameDesc, {
            let store = store.clone();
            move || store.dispatch(Action::SetSort(SortMode::NameDesc))
        }),
        Space(Modifier::new().width(6.0)),
        chip("Popular", s.sort == SortMode::Popularity, {
            let store = store.clone();
            move || store.dispatch(Action::SetSort(SortMode::Popularity))
        }),
    ])
}

fn result_list(store: &Rc<Store>, s: &state::AppState) -> View {
    if s.results.is_empty() {
        return empty_state("🔍", "No results", "Try searching for a package");
    }

    let store = store.clone();
    let selected = s.selected.clone();
    let upgrades_mode = s.in_upgrades_view;
    let results = s.results.clone();

    LazyColumn(
        results,
        72.0,
        remember_with_key("scroll", || LazyColumnState::new()),
        Modifier::new().fill_max_width().height(650.0),
        move |pkg: PackageSummary, _| {
            let is_selected = selected.as_ref().map_or(false, |id| *id == pkg.id);
            pkg_row(store.clone(), pkg, is_selected, upgrades_mode)
        },
    )
}

fn status_bar(store: &Rc<Store>, s: &state::AppState) -> View {
    let last_line = s.progress_log.lines().last().unwrap_or("Ready");

    Row(Modifier::new()
        .padding(16.0)
        .margin_vertical(8.0)
        .background_brush(h_gradient(CARD_START, CARD_END))
        .clip_rounded(CORNER_MD)
        .border(1.0, Color::from_hex(CARD_BORDER), CORNER_MD))
    .child((
        Text("●")
            .size(10.0)
            .color(Color::from_hex(STATUS_DOT))
            .modifier(Modifier::new().padding(4.0)),
        Text("Status")
            .size(SMALL_FONT)
            .color(Color::from_hex(TEXT_MUTED))
            .modifier(Modifier::new().padding(4.0)),
        Text(format!("  {last_line}"))
            .size(SMALL_FONT)
            .color(Color::from_hex(TEXT_SECONDARY))
            .modifier(Modifier::new().padding(4.0)),
        Spacer(),
        secondary_button(
            if s.log_expanded {
                "▼ Hide Log"
            } else {
                "▶ Show Log"
            },
            {
                let store = store.clone();
                move || store.dispatch(Action::ToggleLog)
            },
        ),
    ))
}

fn log_panel(s: &state::AppState) -> View {
    if !s.log_expanded {
        return Box(Modifier::new());
    }

    Box(Modifier::new()
        .fill_max_width()
        .height(200.0)
        .margin(12.0)
        .padding(16.0)
        .background_brush(v_gradient(CARD_END, BG_START))
        .clip_rounded(CORNER_MD)
        .border(1.0, Color::from_hex(CARD_BORDER), CORNER_MD))
    .child(
        Text(s.progress_log.clone())
            .size(12.0)
            .color(Color::from_hex(LOG_TEXT)),
    )
}

pub fn root_view(store: Rc<Store>) -> View {
    let s = store.state.get();

    Surface(
        Modifier::new()
            .fill_max_size()
            .background_brush(v_gradient(BG_START, BG_END)),
        Column(Modifier::new().padding(20.0)).child((
            // Error banner (if any)
            if let Some(err) = s.error.clone() {
                error_banner(&store, err)
            } else {
                Box(Modifier::new())
            },
            header_bar(&store, &s),
            separator(),
            search_bar(&store, &s),
            Space(Modifier::new().height(16.0)),
            Grid(
                6,
                Modifier::new().fill_max_size().padding(8.0),
                vec![
                    Column(Modifier::new().grid_span(4, 1).padding(8.0))
                        .child(result_list(&store, &s)),
                    Column(Modifier::new().grid_span(2, 1).padding(8.0))
                        .child(details_card(store.clone())),
                ],
                12.0,
                16.0,
            ),
            status_bar(&store, &s),
            log_panel(&s),
        )),
    )
}
