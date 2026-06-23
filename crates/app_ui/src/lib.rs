use crate::state::{Action, AppState, SortMode, Store};
use crate::theme::*;
use crate::widgets::*;
use domain::{PackageDetails, PackageSummary, Source};
use repose_core::*;
use repose_core::{
    input::{Key, Modifiers},
    shortcuts::{self, ShortcutMap},
};
use repose_material::material3::{
    ButtonConfig, Divider, DividerConfig, FilledButton, LinearProgressIndicator,
    LinearProgressIndicatorConfig, ListItem, ListItemConfig, OutlinedButton,
    OutlinedTextField, OutlinedTextFieldConfig, SegmentedButton, SegmentedButtonConfig, Segment,
    Surface, SurfaceConfig, TopAppBar, TopAppBarConfig,
};
use repose_ui::{
    lazy::{LazyColumn, LazyColumnState},
    scroll::{ScrollArea, remember_scroll_state},
    *,
};
use repose_material::{Icon, Symbol};
use std::rc::Rc;

pub mod state;
pub mod theme;
pub mod widgets;

const PANE_HEIGHT_DP: f32 = 520.0;

fn top_bar(store: &Rc<Store>, s: &AppState) -> View {
    TopAppBar(
        if s.in_upgrades_view { "Upgrades" } else { "Soredowe" },
        None,
        vec![
            if s.in_upgrades_view && !s.results.is_empty() {
                let content = Row(Modifier::new().align_items(AlignItems::Center).gap(8.0)).child((
                    Icon(Symbol::new("", '\u{F090}')),
                    Text("Upgrade all").size(14.0),
                ));
                FilledButton(Modifier::new(), {
                    let store = store.clone();
                    move || store.dispatch(Action::UpgradeAll)
                }, ButtonConfig::default(), move || content)
            } else {
                Box(Modifier::new())
            },
            {
                let content = Row(Modifier::new().align_items(AlignItems::Center).gap(8.0)).child((
                    Icon(Symbol::new("", '\u{E5D5}')),
                    Text("Refresh").size(14.0),
                ));
                OutlinedButton(Modifier::new(), {
                    let store = store.clone();
                    move || store.dispatch(Action::Refresh)
                }, ButtonConfig::default(), move || content)
            },
            {
                let content = Row(Modifier::new().align_items(AlignItems::Center).gap(8.0)).child((
                    Icon(Symbol::new("", '\u{E8D7}')),
                    Text("Upgrades").size(14.0),
                ));
                FilledButton(Modifier::new(), {
                    let store = store.clone();
                    move || store.dispatch(Action::Upgrades)
                }, ButtonConfig::default(), move || content)
            },
        ],
        TopAppBarConfig::default(),
    )
}

fn search_section(store: &Rc<Store>, s: &AppState) -> View {
    Column(Modifier::new().fill_max_width().padding(16.0))
    .child(vec![
        Row(Modifier::new().fill_max_width()).child((
            OutlinedTextField(
                Modifier::new().flex_grow(1.0).max_width(500.0),
                s.query.clone(),
                {
                    let store = store.clone();
                    move |text| store.dispatch(Action::SetQuery(text))
                },
                OutlinedTextFieldConfig {
                    placeholder: Some("Search packages…".into()),
                    single_line: true,
                    leading_icon: Some(Icon(Symbol::new("search", '\u{E8B6}'))),
                    on_submit: Some(Rc::new({
                        let store = store.clone();
                        move |text| {
                            store.dispatch(Action::SetQuery(text));
                            store.dispatch(Action::Search);
                        }
                    })),
                    ..Default::default()
                },
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
            SegmentedButton(
                &match s.sort {
                    SortMode::Popularity => vec![0],
                    SortMode::NameAsc => vec![1],
                    SortMode::NameDesc => vec![2],
                },
                vec![
                    Segment {
                        label: "Pop".into(),
                        icon: None,
                        on_click: Rc::new({
                            let store = store.clone();
                            move || store.dispatch(Action::SetSort(SortMode::Popularity))
                        }),
                    },
                    Segment {
                        label: "A-Z".into(),
                        icon: None,
                        on_click: Rc::new({
                            let store = store.clone();
                            move || store.dispatch(Action::SetSort(SortMode::NameAsc))
                        }),
                    },
                    Segment {
                        label: "Z-A".into(),
                        icon: None,
                        on_click: Rc::new({
                            let store = store.clone();
                            move || store.dispatch(Action::SetSort(SortMode::NameDesc))
                        }),
                    },
                ],
                SegmentedButtonConfig::default(),
            ),
        ]),
    ])
}

fn pkg_action(store: &Rc<Store>, pkg: &PackageSummary, upgrades_mode: bool) -> View {
    let id = pkg.id.clone();
    let store = store.clone();
    if upgrades_mode {
        success_button("Upgrade", move || store.dispatch(Action::Upgrade(id.clone())))
    } else if pkg.installed {
        danger_button("Remove", move || store.dispatch(Action::Remove(id.clone())))
    } else {
        success_button("Install", move || store.dispatch(Action::Install(id.clone())))
    }
}

fn pkg_row(store: Rc<Store>, pkg: PackageSummary, selected: bool, upgrades_mode: bool) -> View {
    let is_aur = pkg.id.source == Source::Aur;
    let th = theme();
    let trailing = Column(Modifier::new().align_items(AlignItems::FlexEnd)).child((
        Text(pkg.version.clone())
            .size(11.0)
            .color(th.on_surface_variant)
            .modifier(Modifier::new().padding_values(PaddingValues {
                left: 0.0, right: 0.0, top: 0.0, bottom: 4.0,
            })),
        pkg_action(&store, &pkg, upgrades_mode),
    ));

    let bg = if selected {
        th.primary_container
    } else if is_aur {
        th.tertiary_container
    } else {
        th.surface_container_low
    };

    Box(Modifier::new()
        .fill_max_width()
        .background(bg)
        .margin_vertical(3.0)
        .clip_rounded(R_MD))
    .child(
        ListItem(
            pkg.id.name.clone(),
            Some(pkg.description.clone()),
            Some(source_badge(is_aur).modifier(Modifier::new().align_self_center())),
            Some(trailing),
            Some(Rc::new({
                let store = store.clone();
                let id = pkg.id.clone();
                move || store.dispatch(Action::Select(id.clone()))
            })),
            ListItemConfig {
                modifier: Modifier::new()
                    .fill_max_width()
                    .border(1.0, if selected { th.primary } else { th.outline_variant }, R_MD),
                ..Default::default()
            },
        ),
    )
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

fn details_pane(store: Rc<Store>, s: &AppState) -> View {
    let th = theme();

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
                .size(20.0)
                .color(th.on_surface),
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
            .size(13.0)
            .color(th.on_surface_variant),
        Space(Modifier::new().height(10.0)),
        Text(summary.description.clone())
            .size(14.0)
            .color(th.on_surface)
            .max_lines(6)
            .overflow_ellipsize(),
    ));

    let actions = Row(Modifier::new().padding_values(PaddingValues {
        left: 0.0, right: 0.0, top: 12.0, bottom: 4.0,
    }))
    .child(vec![
        pkg_action(&store, &summary, s.in_upgrades_view),
        Space(Modifier::new().width(8.0)),
        text_button("Clear", {
            let store = store.clone();
            move || store.dispatch(Action::ClearSelection)
        }),
    ]);

    let detail_section: View = if let Some(det) = &s.detail {
        details_body(det)
    } else {
        Text("Loading details…")
            .size(13.0)
            .color(th.on_surface_variant)
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
            .background(th.surface)
            .border(1.0, th.outline_variant, R_LG)
            .clip_rounded(R_LG),
        scroll,
        Column(Modifier::new().fill_max_width().padding(16.0)).child((
            header,
            Divider(DividerConfig::default()),
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
        detail_row("Install size", &det.size_install.map(|b| format_bytes(b)).unwrap_or_default()),
        detail_row("Download", &det.size_download.map(|b| format_bytes(b)).unwrap_or_default()),
        tag_list("Dependencies", &det.depends),
        tag_list("Optional deps", &det.opt_depends),
    ))
}

fn status_bar(store: &Rc<Store>, s: &AppState) -> View {
    let th = theme();
    let last = s.progress_log.lines().last().unwrap_or("Ready");

    let indicator = if s.active_stage.is_some() {
        LinearProgressIndicator(
            s.progress_pct,
            LinearProgressIndicatorConfig {
                color: th.primary,
                track_color: th.outline_variant,
                ..Default::default()
            },
        )
    } else {
        Box(Modifier::new())
    };

    Column(Modifier::new().fill_max_width().margin_vertical(4.0)).child((
        indicator,
        Row(Modifier::new()
            .fill_max_width()
            .padding(10.0)
            .background(th.surface_container_low)
            .border(1.0, th.outline_variant, R_MD)
            .clip_rounded(R_MD))
        .child((
            Text("\u{25CF}")
                .size(8.0)
                .color(th.primary)
                .modifier(Modifier::new().align_self_center().padding(4.0)),
            Text(last.to_string())
                .size(13.0)
                .color(th.on_surface_variant)
                .modifier(Modifier::new().flex_grow(1.0).align_self_center()),
            text_button(
                if s.log_expanded { "Hide log" } else { "Show log" },
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

    let th = theme();
    let scroll = remember_scroll_state("log_panel");

    ScrollArea(
        Modifier::new()
            .fill_max_width()
            .fill_max_height()
            .padding(12.0)
            .margin_vertical(4.0)
            .background(th.surface)
            .border(1.0, th.outline_variant, R_MD)
            .clip_rounded(R_MD),
        scroll,
        Column(Modifier::new().fill_max_width()).child(
            Text(s.progress_log.clone())
                .size(12.0)
                .color(th.on_surface_variant)
                .modifier(Modifier::new().fill_max_width()),
        ),
    )
}

fn setup_shortcuts(store: &Rc<Store>) {
    let mut map = ShortcutMap::new();
    map.insert(
        Key::Character('f'),
        Modifiers { ctrl: true, ..Modifiers::default() },
        shortcuts::Action::Custom("search".into()),
    );
    map.insert(
        Key::Character('q'),
        Modifiers { ctrl: true, ..Modifiers::default() },
        shortcuts::Action::Custom("quit".into()),
    );
    scoped_effect({
        let store = store.clone();
        move || {
            let map_scope = shortcuts::InstallShortcutMap(map);
            let handler_scope = shortcuts::InstallShortcutHandler(Rc::new(move |action| {
                match &action {
                    shortcuts::Action::Custom(key) if key.as_ref() == "search" => {
                        store.dispatch(Action::SetQuery(String::new()));
                        true
                    }
                    shortcuts::Action::Custom(key) if key.as_ref() == "quit" => {
                        std::process::exit(0);
                    }
                    _ => false,
                }
            }));
            Dispose::new(move || {
                map_scope.run();
                handler_scope.run();
            })
        }
    });
}

pub fn root_view(store: Rc<Store>) -> View {
    let s = store.state.get();

    Surface(
        SurfaceConfig {
            modifier: Modifier::new()
                .fill_max_size()
                .background_brush(v_gradient(BG_START, BG_END)),
            ..Default::default()
        },
        {
            let store = store.clone();
            let s = s.clone();
            move || {
                setup_shortcuts(&store);

                Column(Modifier::new().fill_max_size()).child(vec![
                    top_bar(&store, &s),
                    Divider(DividerConfig::default()),
                    Column(Modifier::new().fill_max_width().flex_grow(1.0).padding(16.0)).child(vec![
                        search_section(&store, &s),
                        Space(Modifier::new().height(8.0)),
                        Row(Modifier::new()
                            .fill_max_width()
                            .flex_grow(1.0)
                            .flex_basis(0.0))
                        .child((
                            Box(Modifier::new().weight(7.0).padding(4.0)).child(results_list(&store, &s)),
                            Space(Modifier::new().width(8.0)),
                            Box(Modifier::new().weight(3.0).padding(4.0))
                                .child(details_pane(store.clone(), &s)),
                        )),
                        status_bar(&store, &s),
                        log_panel(&s),
                    ]),
                ])
            }
        },
    )
}
