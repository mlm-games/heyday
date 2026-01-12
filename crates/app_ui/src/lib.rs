use crate::state::{Action, SortMode, Store};
use domain::{PackageSummary, Source};
use repose_core::*;
use repose_ui::{
    lazy::{LazyColumn, LazyColumnState},
    *,
};
use std::rc::Rc;

pub mod state;

// Simple badges
fn badge(text: &str, bg: Color) -> View {
    Text(text.to_string())
        .color(Color::from_hex("#EEEEEE"))
        .modifier(
            Modifier::new()
                .padding(2.0)
                .background(bg)
                .clip_rounded(4.0)
                .padding(6.0),
        )
}

// Filter chip
fn chip(label: &str, on: bool, on_toggle: impl Fn() + 'static) -> View {
    Button(Text(label), on_toggle).modifier(
        Modifier::new()
            .padding(4.0)
            .background(if on {
                Color::from_hex("#2A8F6A")
            } else {
                Color::from_hex("#2A2A2A")
            })
            .clip_rounded(6.0),
    )
}

// Row separator
fn separator() -> View {
    Box(Modifier::new()
        .size(1.0, 1.0)
        .background(Color::from_hex("#2A2A2A")))
}

// Package row
fn pkg_row(store: Rc<Store>, pkg: PackageSummary, selected: bool, upgrades_mode: bool) -> View {
    let is_aur = pkg.id.source == Source::Aur;
    Row(Modifier::new()
        .padding(10.0)
        .background(if selected {
            Color::from_hex("#244E74")
        } else if is_aur {
            Color::from_hex("#1A2030")
        } else {
            Color::from_hex("#1E1E1E")
        })
        .border(1.0, Color::from_hex("#333333"), 8.0)
        .clip_rounded(8.0)
        .clickable()
        .on_pointer_down({
            let store = store.clone();
            let id = pkg.id.clone();
            move |_| store.dispatch(Action::Select(id.clone()))
        }))
    .child((
        Column(Modifier::new().flex_grow(1.0)).child((
            Row(Modifier::new()).child((
                Text(pkg.id.name.clone()).modifier(Modifier::new().padding(2.0)),
                if is_aur {
                    badge("AUR", Color::from_hex("#6B46C1"))
                } else {
                    badge("Repo", Color::from_hex("#2D6A4F"))
                },
                if pkg.installed {
                    badge("Installed", Color::from_hex("#4B5563"))
                } else {
                    Box(Modifier::new())
                },
            )),
            Text(pkg.description.clone())
                .size(12.0)
                .color(Color::from_hex("#AAAAAA"))
                .max_lines(1)
                .overflow_ellipsize()
                .modifier(Modifier::new().padding(2.0).flex_grow(1.0).max_width(500.0)),
        )),
        if upgrades_mode {
            Button(Text("Upgrade"), {
                let store = store.clone();
                let id = pkg.id.clone();
                move || store.dispatch(Action::Upgrade(id.clone()))
            })
        } else {
            let label = if pkg.installed { "Remove" } else { "Install" };
            Button(Text(label), {
                let store = store.clone();
                let id = pkg.id.clone();
                let installed = pkg.installed;
                move || {
                    if installed {
                        store.dispatch(Action::Remove(id.clone()))
                    } else {
                        store.dispatch(Action::Install(id.clone()))
                    }
                }
            })
        },
    ))
}

// Details card (right pane)
fn details_card(store: Rc<Store>) -> View {
    let s = store.state.get();
    let results = s.results.clone();
    let Some(id) = &s.selected else {
        return Column(Modifier::new().padding(16.0))
            .child(Text("Select a package to see details").color(Color::from_hex("#AAAAAA")));
    };
    // Find summary in current results (lightweight until details endpoint is used)
    let pkg = results.into_iter().find(|p| &p.id == id);
    if let Some(pkg) = pkg {
        Column(
            Modifier::new()
                .padding(16.0)
                .background(Color::from_hex("#1B1B1B"))
                .border(1.0, Color::from_hex("#333333"), 10.0)
                .clip_rounded(10.0),
        )
        .child((
            Row(Modifier::new().align_self_center()).child((
                Text(pkg.id.name.clone()).size(18.0),
                if pkg.id.source == Source::Aur {
                    badge("AUR", Color::from_hex("#6B46C1"))
                } else {
                    badge("Repo", Color::from_hex("#2D6A4F"))
                },
                if pkg.installed {
                    badge("Installed", Color::from_hex("#4B5563"))
                } else {
                    Box(Modifier::new())
                },
            )),
            Text(pkg.description.clone())
                .max_lines(10)
                .overflow_clip()
                .color(Color::from_hex("#BBBBBB"))
                .modifier(Modifier::new().padding(6.0)),
            Row(Modifier::new().padding(8.0)).child((
                Spacer(),
                if s.in_upgrades_view {
                    Button(Text("Upgrade"), {
                        let store = store.clone();
                        let id = pkg.id.clone();
                        move || store.dispatch(Action::Upgrade(id.clone()))
                    })
                } else {
                    let label = if pkg.installed { "Remove" } else { "Install" };
                    Button(Text(label), {
                        let store = store.clone();
                        let id = pkg.id.clone();
                        let installed = pkg.installed;
                        move || {
                            if installed {
                                store.dispatch(Action::Remove(id.clone()))
                            } else {
                                store.dispatch(Action::Install(id.clone()))
                            }
                        }
                    })
                },
                Spacer(),
                Button(Text("Clear selection"), {
                    let store = store.clone();
                    move || store.dispatch(Action::ClearSelection)
                }),
                Spacer(),
            )),
        ))
    } else {
        Column(Modifier::new().padding(16.0))
            .child(Text("No details available").color(Color::from_hex("#AAAAAA")))
    }
}

pub fn root_view(store: Rc<Store>) -> View {
    let s = store.state.get();

    let _current_query = s.query.clone();

    Surface(
        Modifier::new()
            .fill_max_size()
            .background(Color::from_hex("#0F1012")),
        Column(Modifier::new().padding(12.0)).child((
            // Error banner
            if let Some(err) = s.error.clone() {
                Row(Modifier::new()
                    .padding(8.0)
                    .background(Color::from_hex("#3B1F1F"))
                    .border(1.0, Color::from_hex("#7F1D1D"), 8.0)
                    .clip_rounded(8.0))
                .child((
                    Text("⚠ ").color(Color::from_hex("#FCA5A5")).size(16.0),
                    Text(err)
                        .color(Color::from_hex("#FCA5A5"))
                        .modifier(Modifier::new().flex_grow(1.0).padding(4.0)),
                    Button(Text("Dismiss"), {
                        let store = store.clone();
                        move || store.dispatch(Action::ClearError)
                    }),
                ))
            } else {
                Box(Modifier::new())
            },
            // Header bar
            Row(Modifier::new().padding(8.0)).child((
                Text("soredowe")
                    .size(20.0)
                    .modifier(Modifier::new().padding(8.0)),
                Spacer(),
                if s.in_upgrades_view && !s.results.is_empty() {
                    Button(Text("Upgrade all"), {
                        let store = store.clone();
                        move || store.dispatch(Action::UpgradeAll)
                    })
                    .modifier(Modifier::new().padding(4.0))
                } else {
                    Box(Modifier::new())
                },
                Button(Text("🔃 Refresh"), {
                    let store = store.clone();
                    move || store.dispatch(Action::Refresh)
                })
                .modifier(Modifier::new().padding(4.0)),
                Button(Text("Upgrades"), {
                    let store = store.clone();
                    move || store.dispatch(Action::Upgrades)
                })
                .modifier(Modifier::new().padding(4.0)),
            )),
            separator(),
            // Search row
            Row(Modifier::new().padding(8.0)).child((
                repose_ui::textfield::TextField(
                    "Search packages…",
                    Modifier::new()
                        .size(420.0, 36.0)
                        .background(Color::from_hex("#171717"))
                        .border(1.0, Color::from_hex("#3A3A3A"), 6.0)
                        .clip_rounded(6.0)
                        .semantics(Semantics {
                            role: Role::TextField,
                            label: Some("Search field".into()),
                            focused: false,
                            enabled: true,
                        }),
                    Some({
                        let store = store.clone();
                        move |text: String| {
                            // Update store's query on every keystroke
                            store.dispatch(Action::SetQuery(text));
                        }
                    }),
                    Some({
                        let store = store.clone();
                        move |text: String| {
                            // On Enter: set query and search
                            store.dispatch(Action::SetQuery(text));
                            store.dispatch(Action::Search);
                        }
                    }),
                ),
                // Search button - uses query from store
                Button(Text("Search"), {
                    let store = store.clone();
                    move || {
                        store.dispatch(Action::Search);
                    }
                })
                .modifier(Modifier::new().padding(4.0)),
                // Filters
                chip("Repo", s.filter_repo, {
                    let store = store.clone();
                    move || store.dispatch(Action::ToggleFilterRepo)
                }),
                chip("AUR", s.filter_aur, {
                    let store = store.clone();
                    move || store.dispatch(Action::ToggleFilterAur)
                }),
                chip("Installed", s.filter_installed, {
                    let store = store.clone();
                    move || store.dispatch(Action::ToggleFilterInstalled)
                }),
                Spacer(),
                // Sort
                Row(Modifier::new().padding(6.0)).child((
                    Button(Text("A–Z"), {
                        let store = store.clone();
                        move || store.dispatch(Action::SetSort(SortMode::NameAsc))
                    }),
                    Button(Text("Z–A"), {
                        let store = store.clone();
                        move || store.dispatch(Action::SetSort(SortMode::NameDesc))
                    }),
                    Button(Text("Popular"), {
                        let store = store.clone();
                        move || store.dispatch(Action::SetSort(SortMode::Popularity))
                    }),
                )),
            )),
            {
                let wide = true;
                let left_span = if wide { 4 } else { 6 };
                let right_span = if wide { 2 } else { 6 };

                Grid(
                    6,
                    Modifier::new().fill_max_size().padding(6.0),
                    vec![
                        // Left: result list
                        Column(Modifier::new().grid_span(left_span, 1)).child(
                            if s.results.is_empty() {
                                Column(Modifier::new().padding(16.0)).child(
                                    Text("No results. Try searching.")
                                        .color(Color::from_hex("#888888")),
                                )
                            } else {
                                LazyColumn(
                                    s.results.clone(),
                                    56.0,
                                    remember_with_key("scroll", || LazyColumnState::new()),
                                    Modifier::new().fill_max_width().height(700.0),
                                    {
                                        let store = store.clone();
                                        let upgrades_mode = s.in_upgrades_view;
                                        move |pkg: PackageSummary, _| {
                                            let selected = s
                                                .selected
                                                .as_ref()
                                                .map_or(false, |id| *id == pkg.id);
                                            pkg_row(store.clone(), pkg, selected, upgrades_mode)
                                        }
                                    },
                                )
                            },
                        ),
                        // Right: details
                        Column(Modifier::new().grid_span(right_span, 1))
                            .child(details_card(store.clone())),
                    ],
                    6.0,
                    6.0,
                )
            },
            // Footer / status
            Row(Modifier::new().padding(8.0)).child((
                Text("Status").size(12.0).color(Color::from_hex("#888888")),
                Text(format!(
                    "  |  {}",
                    s.progress_log.lines().last().unwrap_or("")
                ))
                .color(Color::from_hex("#A0A0A0"))
                .modifier(Modifier::new().padding(4.0)),
                Spacer(),
                Button(
                    Text(if s.log_expanded {
                        "Hide log"
                    } else {
                        "Show log"
                    }),
                    {
                        let store = store.clone();
                        move || store.dispatch(Action::ToggleLog)
                    },
                ),
            )),
            if s.log_expanded {
                Box(Modifier::new()
                    .fill_max_size()
                    .size(0.0, 180.0)
                    .background(Color::TRANSPARENT)
                    .clip_rounded(6.0))
                .child(
                    Text(s.progress_log.clone())
                        .size(12.0)
                        .color(Color::from_hex("#B0B0B0"))
                        .modifier(Modifier::new().padding(8.0)),
                )
            } else {
                Box(Modifier::new())
            },
        )),
    )
}
