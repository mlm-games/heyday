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
    AlertDialog, ButtonConfig, Divider, DividerConfig, FilledButton,
    LinearProgressIndicator, LinearProgressIndicatorConfig, ListItem, ListItemConfig,
    OutlinedTextField, OutlinedTextFieldConfig, SegmentedButton, SegmentedButtonConfig, Segment,
    Surface, SurfaceConfig,
};
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

fn top_bar(store: &Rc<Store>, s: &AppState) -> View {
    let th = theme();
    Row(Modifier::new()
        .fill_max_width()
        .padding_values(PaddingValues { left: 16.0, right: 16.0, top: 8.0, bottom: 8.0 })
        .background(th.surface_container))
    .child(vec![
        Text(if s.in_upgrades_view { "Upgrades" } else { "Soredowe" })
            .size(22.0)
            .color(th.on_surface)
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
        Space(Modifier::new().width(6.0)),
        outline_button("Install .pkg", {
            let store = store.clone();
            move || store.dispatch(Action::InstallLocal)
        }),
        Space(Modifier::new().width(6.0)),
        outline_button("Refresh", {
            let store = store.clone();
            move || store.dispatch(Action::Refresh)
        }),
        Space(Modifier::new().width(6.0)),
        FilledButton(Modifier::new(), {
            let store = store.clone();
            move || store.dispatch(Action::Upgrades)
        }, ButtonConfig::default(), || Text("Upgrades").size(14.0)),
        Space(Modifier::new().width(6.0)),
        outline_button("Cache", {
            let store = store.clone();
            move || store.dispatch(Action::CleanCache(3))
        }),
        Space(Modifier::new().width(6.0)),
        outline_button("Orphans", {
            let store = store.clone();
            move || store.dispatch(Action::ShowOrphans)
        }),
        Space(Modifier::new().width(6.0)),
        outline_button("Export", {
            let store = store.clone();
            move || store.dispatch(Action::ShowExport)
        }),
        Space(Modifier::new().width(6.0)),
        outline_button("Verify", {
            let store = store.clone();
            move || store.dispatch(Action::ShowVerify)
        }),
        Space(Modifier::new().width(6.0)),
        outline_button(".pacnew", {
            let store = store.clone();
            move || store.dispatch(Action::ShowPacnew)
        }),
    ])
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
        if summary.installed {
            outline_button("Downgrade", {
                let store = store.clone();
                let id = summary.id.clone();
                move || store.dispatch(Action::ShowDowngrade(id.clone()))
            })
        } else {
            Box(Modifier::new())
        },
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
        tag_list("Groups", &det.summary.groups),
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

fn pkgbuild_review_dialog(store: &Rc<Store>, s: &AppState) -> View {
    let Some(ref review) = s.pending_pkgbuild_review else {
        return Box(Modifier::new());
    };

    let scroll = remember_scroll_state("pkgbuild_review");

    AlertDialog(
        true,
        || {}, // no dismiss
        Text("Review PKGBUILD").size(20.0),
        Column(Modifier::new()).child(vec![
            Text(format!("Package: {}", review.id.name)).size(13.0),
            Space(Modifier::new().height(12.0)),
            ScrollArea(
                Modifier::new()
                    .fill_max_width()
                    .max_height(400.0)
                    .padding(12.0),
                scroll,
                Text(review.content.clone()).size(12.0).modifier(Modifier::new().fill_max_width()),
            ),
        ]),
        Row(Modifier::new()).child((
            danger_button("Reject", {
                let store = store.clone();
                move || store.dispatch(Action::RejectPkgbuild)
            }),
            Space(Modifier::new().width(8.0)),
            success_button("Approve", {
                let store = store.clone();
                move || store.dispatch(Action::ApprovePkgbuild)
            }),
        )),
        None,
    )
}

fn history_panel(s: &AppState) -> View {
    if s.history.is_empty() {
        return Box(Modifier::new());
    }

    let th = theme();
    let scroll = remember_scroll_state("history_panel");

    Box(Modifier::new()
        .fill_max_width()
        .padding(8.0)
        .background(th.surface)
        .border(1.0, th.outline_variant, R_MD)
        .clip_rounded(R_MD))
    .child(ScrollArea(
        Modifier::new()
            .fill_max_width()
            .max_height(200.0)
            .padding(4.0),
        scroll,
        Column(Modifier::new().fill_max_width()).child(
            s.history
                .iter()
                .map(|entry| {
                    let status_color = if entry.success { th.primary } else { th.error };
                    Row(
                        Modifier::new()
                            .fill_max_width()
                            .padding(4.0)
                            .margin_vertical(2.0),
                    )
                    .child(vec![
                        Text(entry.kind.clone())
                            .size(11.0)
                            .color(th.on_surface_variant)
                            .modifier(Modifier::new().width(70.0)),
                        Text(entry.pkg.clone().unwrap_or_default())
                            .size(11.0)
                            .color(th.on_surface_variant)
                            .modifier(Modifier::new().width(120.0)),
                        Text(if entry.success { "OK" } else { "FAIL" })
                            .size(11.0)
                            .color(status_color),
                        Text(entry.message.clone())
                            .size(11.0)
                            .color(th.on_surface_variant)
                            .modifier(Modifier::new().flex_grow(1.0).max_width(200.0))
                            .overflow_ellipsize(),
                    ])
                })
                .collect::<Vec<_>>(),
        ),
    ))
}

fn orphans_dialog(store: &Rc<Store>, s: &AppState) -> View {
    let scroll = remember_scroll_state("orphans_dialog");

    AlertDialog(
        true,
        {
            let store = store.clone();
            move || store.dispatch(Action::HideOrphans)
        },
        Text("Orphaned Packages").size(20.0),
        Column(Modifier::new()).child(vec![
            Text("Packages installed as dependencies but no longer required.")
                .size(13.0),
            Space(Modifier::new().height(12.0)),
            if s.orphans.is_empty() {
                Box(Modifier::new())
            } else {
                ScrollArea(
                    Modifier::new()
                        .fill_max_width()
                        .max_height(400.0)
                        .padding(8.0),
                    scroll,
                    Column(Modifier::new().fill_max_width()).child(
                        s.orphans
                            .iter()
                            .map(|pkg| {
                                Row(Modifier::new()
                                    .fill_max_width()
                                    .padding(8.0)
                                    .margin_vertical(2.0))
                                .child(vec![
                                    Column(Modifier::new().flex_grow(1.0)).child((
                                        Text(pkg.id.name.clone()).size(14.0),
                                        Text(pkg.description.clone())
                                            .size(11.0)
                                            .max_lines(1)
                                            .overflow_ellipsize(),
                                    )),
                                    danger_button("Remove", {
                                        let store = store.clone();
                                        let id = pkg.id.clone();
                                        move || store.dispatch(Action::RemoveOrphan(id.clone()))
                                    }),
                                ])
                            })
                            .collect::<Vec<_>>(),
                    ),
                )
            },
        ]),
        text_button("Close", {
            let store = store.clone();
            move || store.dispatch(Action::HideOrphans)
        }),
        None,
    )
}

fn export_panel(store: &Rc<Store>, text: &str) -> View {
    let th = theme();
    let scroll = remember_scroll_state("export_panel");
    let text = text.to_string();

    Column(
        Modifier::new()
            .fill_max_width()
            .padding(8.0)
            .background(th.surface)
            .border(1.0, th.outline_variant, R_MD)
            .clip_rounded(R_MD),
    )
    .child(vec![
        Row(Modifier::new().fill_max_width()).child((
            Text("Package Export")
                .size(13.0)
                .color(th.on_surface),
            Spacer(),
            text_button("Dismiss", {
                let store = store.clone();
                move || store.dispatch(Action::ShowExport)
            }),
        )),
        Space(Modifier::new().height(4.0)),
        ScrollArea(
            Modifier::new()
                .fill_max_width()
                .max_height(150.0)
                .padding(8.0)
                .background(th.surface_container)
                .border(1.0, th.outline_variant, R_MD)
                .clip_rounded(R_MD),
            scroll,
            Text(text)
                .size(11.0)
                .color(th.on_surface_variant)
                .modifier(Modifier::new().fill_max_width()),
        ),
    ])
}

fn pacnew_panel(store: &Rc<Store>, s: &AppState) -> View {
    let th = theme();
    let scroll = remember_scroll_state("pacnew_panel");

    Column(
        Modifier::new()
            .fill_max_width()
            .padding(8.0)
            .background(th.surface)
            .border(1.0, th.outline_variant, R_MD)
            .clip_rounded(R_MD),
    )
    .child(vec![
        Row(Modifier::new().fill_max_width()).child((
            Text(format!(".pacnew Files ({})", s.pacnew_files.len()))
                .size(13.0)
                .color(th.on_surface),
            Spacer(),
            text_button("Dismiss", {
                let store = store.clone();
                move || store.dispatch(Action::HidePacnew)
            }),
        )),
        Space(Modifier::new().height(4.0)),
        ScrollArea(
            Modifier::new()
                .fill_max_width()
                .max_height(150.0)
                .padding(4.0),
            scroll,
            Column(Modifier::new().fill_max_width()).child(
                s.pacnew_files
                    .iter()
                    .map(|f| {
                        Row(Modifier::new()
                            .fill_max_width()
                            .padding(4.0)
                            .margin_vertical(1.0))
                        .child((
                            Text(f.package.clone())
                                .size(11.0)
                                .color(th.on_surface_variant)
                                .modifier(Modifier::new().width(100.0)),
                            Text(f.path.clone())
                                .size(11.0)
                                .color(th.on_surface_variant)
                                .overflow_ellipsize()
                                .modifier(Modifier::new().flex_grow(1.0)),
                        ))
                    })
                    .collect::<Vec<_>>(),
            ),
        ),
    ])
}

fn verify_panel(store: &Rc<Store>, text: &str) -> View {
    let th = theme();
    let scroll = remember_scroll_state("verify_panel");
    let text = text.to_string();
    let is_ok = text == "All packages verified OK.";

    Column(
        Modifier::new()
            .fill_max_width()
            .padding(8.0)
            .background(th.surface)
            .border(1.0, th.outline_variant, R_MD)
            .clip_rounded(R_MD),
    )
    .child(vec![
        Row(Modifier::new().fill_max_width()).child((
            Text("Verification Results")
                .size(13.0)
                .color(th.on_surface),
            Spacer(),
            text_button("Dismiss", {
                let store = store.clone();
                move || store.dispatch(Action::ShowVerify)
            }),
        )),
        Space(Modifier::new().height(4.0)),
        ScrollArea(
            Modifier::new()
                .fill_max_width()
                .max_height(150.0)
                .padding(8.0)
                .background(th.surface_container)
                .border(1.0, th.outline_variant, R_MD)
                .clip_rounded(R_MD),
            scroll,
            Text(text.clone())
                .size(11.0)
                .color(if is_ok { th.primary } else { th.error })
                .modifier(Modifier::new().fill_max_width()),
        ),
    ])
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

    // Dialogs take over the full screen
    if s.pending_pkgbuild_review.is_some() {
        return pkgbuild_review_dialog(&store, &s);
    }
    if s.show_orphans {
        return orphans_dialog(&store, &s);
    }

    Surface(
        SurfaceConfig {
            modifier: Modifier::new().fill_max_size(),
            color: theme().background,
            ..Default::default()
        },
        {
            let store = store.clone();
            let s = s.clone();
            move || {
                setup_shortcuts(&store);

                let content_weight = if s.log_expanded { 7.0 } else { 1.0 };

                let mut children: Vec<View> = vec![
                    top_bar(&store, &s),
                    Divider(DividerConfig::default()),
                    search_section(&store, &s),
                    Space(Modifier::new().height(8.0)),
                    Column(Modifier::new().fill_max_width().flex_grow(1.0))
                    .child((
                        Row(Modifier::new()
                            .fill_max_width()
                            .weight(content_weight)
                            .flex_basis(0.0))
                        .child((
                            Box(Modifier::new().weight(7.0).padding(4.0)).child(results_list(&store, &s)),
                            Space(Modifier::new().width(8.0)),
                            Box(Modifier::new().weight(3.0).padding(4.0))
                                .child(details_pane(store.clone(), &s)),
                        )),
                        if s.log_expanded {
                            Box(Modifier::new().weight(3.0).padding(4.0))
                                .child(log_panel(&s))
                        } else {
                            Box(Modifier::new())
                        },
                    )),
                    status_bar(&store, &s),
                ];
                if !s.history.is_empty() {
                    children.push(history_panel(&s));
                }
                if !s.pacnew_files.is_empty() {
                    children.push(pacnew_panel(&store, &s));
                }
                if let Some(ref text) = s.export_text {
                    children.push(export_panel(&store, text));
                }
                if let Some(ref text) = s.verify_text {
                    children.push(verify_panel(&store, text));
                }
                Column(Modifier::new().fill_max_size().padding(16.0)).child(children)
            }
        },
    )
}
