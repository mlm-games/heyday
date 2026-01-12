use crate::state::{Action, SortMode, Store};
use domain::{PackageSummary, Source};
use repose_core::*;
use repose_ui::{
    lazy::{LazyColumn, LazyColumnState},
    *,
};
use std::rc::Rc;

pub mod state;

fn badge(text: &str, start_color: &str, end_color: &str) -> View {
    Text(text.to_string())
        .color(Color::from_hex("#FFFFFF"))
        .size(11.0)
        .modifier(
            Modifier::new()
                .padding_values(PaddingValues {
                    left: 10.0,
                    right: 10.0,
                    top: 4.0,
                    bottom: 4.0,
                })
                .background_brush(Brush::Linear {
                    start: Vec2 { x: 0.0, y: 0.0 },
                    end: Vec2 { x: 1.0, y: 1.0 },
                    start_color: Color::from_hex(start_color),
                    end_color: Color::from_hex(end_color),
                })
                .clip_rounded(12.0),
        )
}

fn chip(label: &str, on: bool, on_toggle: impl Fn() + 'static) -> View {
    let (start, end) = if on {
        ("#22C55E", "#16A34A")
    } else {
        ("#374151", "#1F2937")
    };

    Button(Text(label).size(13.0), on_toggle).modifier(
        Modifier::new()
            .padding_values(PaddingValues {
                left: 16.0,
                right: 16.0,
                top: 8.0,
                bottom: 8.0,
            })
            .background_brush(Brush::Linear {
                start: Vec2 { x: 0.0, y: 0.0 },
                end: Vec2 { x: 0.0, y: 1.0 },
                start_color: Color::from_hex(start),
                end_color: Color::from_hex(end),
            })
            .clip_rounded(20.0)
            .border(
                1.0,
                if on {
                    Color::from_hex("#34D399")
                } else {
                    Color::from_hex("#4B5563")
                },
                20.0,
            ),
    )
}

fn separator() -> View {
    Box(Modifier::new()
        .height(1.0)
        .fill_max_width()
        .background_brush(Brush::Linear {
            start: Vec2 { x: 0.0, y: 0.0 },
            end: Vec2 { x: 1.0, y: 0.0 },
            start_color: Color::from_hex("#1F2937"),
            end_color: Color::from_hex("#374151"),
        })
        .margin(8.0))
}

fn styled_button(label: &str, on_click: impl Fn() + 'static) -> View {
    Button(Text(label).size(14.0), on_click).modifier(
        Modifier::new()
            .padding_values(PaddingValues {
                left: 20.0,
                right: 20.0,
                top: 10.0,
                bottom: 10.0,
            })
            .background_brush(Brush::Linear {
                start: Vec2 { x: 0.0, y: 0.0 },
                end: Vec2 { x: 0.0, y: 1.0 },
                start_color: Color::from_hex("#3B82F6"),
                end_color: Color::from_hex("#2563EB"),
            })
            .clip_rounded(14.0)
            .border(1.0, Color::from_hex("#60A5FA"), 14.0),
    )
}

fn secondary_button(label: &str, on_click: impl Fn() + 'static) -> View {
    Button(
        Text(label).size(14.0).color(Color::from_hex("#D1D5DB")),
        on_click,
    )
    .modifier(
        Modifier::new()
            .padding_values(PaddingValues {
                left: 20.0,
                right: 20.0,
                top: 10.0,
                bottom: 10.0,
            })
            .background_brush(Brush::Linear {
                start: Vec2 { x: 0.0, y: 0.0 },
                end: Vec2 { x: 0.0, y: 1.0 },
                start_color: Color::from_hex("#374151"),
                end_color: Color::from_hex("#1F2937"),
            })
            .clip_rounded(14.0)
            .border(1.0, Color::from_hex("#4B5563"), 14.0),
    )
}

fn success_button(label: &str, on_click: impl Fn() + 'static) -> View {
    Button(Text(label).size(14.0), on_click).modifier(
        Modifier::new()
            .padding_values(PaddingValues {
                left: 20.0,
                right: 20.0,
                top: 10.0,
                bottom: 10.0,
            })
            .background_brush(Brush::Linear {
                start: Vec2 { x: 0.0, y: 0.0 },
                end: Vec2 { x: 0.0, y: 1.0 },
                start_color: Color::from_hex("#22C55E"),
                end_color: Color::from_hex("#16A34A"),
            })
            .clip_rounded(14.0)
            .border(1.0, Color::from_hex("#34D399"), 14.0),
    )
}

fn danger_button(label: &str, on_click: impl Fn() + 'static) -> View {
    Button(Text(label).size(14.0), on_click).modifier(
        Modifier::new()
            .padding_values(PaddingValues {
                left: 20.0,
                right: 20.0,
                top: 10.0,
                bottom: 10.0,
            })
            .background_brush(Brush::Linear {
                start: Vec2 { x: 0.0, y: 0.0 },
                end: Vec2 { x: 0.0, y: 1.0 },
                start_color: Color::from_hex("#EF4444"),
                end_color: Color::from_hex("#DC2626"),
            })
            .clip_rounded(14.0)
            .border(1.0, Color::from_hex("#F87171"), 14.0),
    )
}

fn pkg_row(store: Rc<Store>, pkg: PackageSummary, selected: bool, upgrades_mode: bool) -> View {
    let is_aur = pkg.id.source == Source::Aur;

    let (bg_start, bg_end, border_color) = if selected {
        ("#1E40AF", "#1E3A8A", "#3B82F6")
    } else if is_aur {
        ("#1E1B4B", "#0F0D24", "#4338CA")
    } else {
        ("#1F2937", "#111827", "#374151")
    };

    Row(Modifier::new()
        .padding(16.0)
        .margin_vertical(8.0)
        .background_brush(Brush::Linear {
            start: Vec2 { x: 0.0, y: 0.0 },
            end: Vec2 { x: 1.0, y: 1.0 },
            start_color: Color::from_hex(bg_start),
            end_color: Color::from_hex(bg_end),
        })
        .border(1.0, Color::from_hex(border_color), 16.0)
        .clip_rounded(16.0)
        .clickable()
        .on_pointer_down({
            let store = store.clone();
            let id = pkg.id.clone();
            move |_| store.dispatch(Action::Select(id.clone()))
        }))
    .child((
        Column(Modifier::new().flex_grow(1.0)).child((
            Row(Modifier::new().padding_values(PaddingValues {
                left: 0.0,
                right: 0.0,
                top: 0.0,
                bottom: 8.0,
            }))
            .child((
                Text(pkg.id.name.clone())
                    .size(16.0)
                    .color(Color::from_hex("#F9FAFB"))
                    .modifier(Modifier::new().padding(4.0)),
                Space(Modifier::new().width(8.0)),
                if is_aur {
                    badge("AUR", "#7C3AED", "#5B21B6")
                } else {
                    badge("Repo", "#059669", "#047857")
                },
                Space(Modifier::new().width(6.0)),
                if pkg.installed {
                    badge("Installed", "#6366F1", "#4F46E5")
                } else {
                    Box(Modifier::new())
                },
            )),
            Text(pkg.description.clone())
                .size(13.0)
                .color(Color::from_hex("#9CA3AF"))
                .max_lines(2)
                .overflow_ellipsize()
                .modifier(Modifier::new().flex_grow(1.0).max_width(500.0)),
        )),
        Space(Modifier::new().width(16.0)),
        if upgrades_mode {
            success_button("Upgrade", {
                let store = store.clone();
                let id = pkg.id.clone();
                move || store.dispatch(Action::Upgrade(id.clone()))
            })
        } else if pkg.installed {
            danger_button("Remove", {
                let store = store.clone();
                let id = pkg.id.clone();
                move || store.dispatch(Action::Remove(id.clone()))
            })
        } else {
            success_button("Install", {
                let store = store.clone();
                let id = pkg.id.clone();
                move || store.dispatch(Action::Install(id.clone()))
            })
        },
    ))
}

fn details_card(store: Rc<Store>) -> View {
    let s = store.state.get();
    let results = s.results.clone();
    let Some(id) = &s.selected else {
        return Column(
            Modifier::new()
                .padding(32.0)
                .fill_max_width()
                .background_brush(Brush::Linear {
                    start: Vec2 { x: 0.0, y: 0.0 },
                    end: Vec2 { x: 1.0, y: 1.0 },
                    start_color: Color::from_hex("#1F2937"),
                    end_color: Color::from_hex("#111827"),
                })
                .clip_rounded(20.0)
                .border(1.0, Color::from_hex("#374151"), 20.0),
        )
        .child((
            Text("📦")
                .size(48.0)
                .modifier(Modifier::new().align_self_center().padding(16.0)),
            Text("Select a package to see details")
                .size(16.0)
                .color(Color::from_hex("#9CA3AF"))
                .modifier(Modifier::new().align_self_center()),
        ));
    };

    let pkg = results.into_iter().find(|p| &p.id == id);
    if let Some(pkg) = pkg {
        Column(
            Modifier::new()
                .padding(24.0)
                .fill_max_width()
                .background_brush(Brush::Linear {
                    start: Vec2 { x: 0.0, y: 0.0 },
                    end: Vec2 { x: 1.0, y: 1.0 },
                    start_color: Color::from_hex("#1F2937"),
                    end_color: Color::from_hex("#111827"),
                })
                .border(1.0, Color::from_hex("#374151"), 20.0)
                .clip_rounded(20.0),
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
                    .size(22.0)
                    .color(Color::from_hex("#F9FAFB")),
                Space(Modifier::new().width(12.0)),
                if pkg.id.source == Source::Aur {
                    badge("AUR", "#7C3AED", "#5B21B6")
                } else {
                    badge("Repo", "#059669", "#047857")
                },
                Space(Modifier::new().width(8.0)),
                if pkg.installed {
                    badge("Installed", "#6366F1", "#4F46E5")
                } else {
                    Box(Modifier::new())
                },
            )),
            // Divider
            Box(Modifier::new()
                .height(1.0)
                .fill_max_width()
                .background(Color::from_hex("#374151"))
                .margin_vertical(16.0)),
            // Description
            Text(pkg.description.clone())
                .max_lines(10)
                .overflow_clip()
                .size(14.0)
                .color(Color::from_hex("#D1D5DB"))
                .modifier(Modifier::new().padding_values(PaddingValues {
                    left: 0.0,
                    right: 0.0,
                    top: 0.0,
                    bottom: 24.0,
                })),
            // Actions
            Row(Modifier::new()).child((
                if s.in_upgrades_view {
                    success_button("Upgrade", {
                        let store = store.clone();
                        let id = pkg.id.clone();
                        move || store.dispatch(Action::Upgrade(id.clone()))
                    })
                } else if pkg.installed {
                    danger_button("Remove", {
                        let store = store.clone();
                        let id = pkg.id.clone();
                        move || store.dispatch(Action::Remove(id.clone()))
                    })
                } else {
                    success_button("Install", {
                        let store = store.clone();
                        let id = pkg.id.clone();
                        move || store.dispatch(Action::Install(id.clone()))
                    })
                },
                Space(Modifier::new().width(12.0)),
                secondary_button("Clear", {
                    let store = store.clone();
                    move || store.dispatch(Action::ClearSelection)
                }),
            )),
        ))
    } else {
        Column(
            Modifier::new()
                .padding(32.0)
                .background_brush(Brush::Linear {
                    start: Vec2 { x: 0.0, y: 0.0 },
                    end: Vec2 { x: 1.0, y: 1.0 },
                    start_color: Color::from_hex("#1F2937"),
                    end_color: Color::from_hex("#111827"),
                })
                .clip_rounded(20.0)
                .border(1.0, Color::from_hex("#374151"), 20.0),
        )
        .child(
            Text("No details available")
                .size(14.0)
                .color(Color::from_hex("#9CA3AF")),
        )
    }
}

pub fn root_view(store: Rc<Store>) -> View {
    let s = store.state.get();

    Surface(
        Modifier::new()
            .fill_max_size()
            .background_brush(Brush::Linear {
                start: Vec2 { x: 0.0, y: 0.0 },
                end: Vec2 { x: 0.0, y: 1.0 },
                start_color: Color::from_hex("#0F172A"),
                end_color: Color::from_hex("#020617"),
            }),
        Column(Modifier::new().padding(20.0)).child((
            // Error banner
            if let Some(err) = s.error.clone() {
                Row(Modifier::new()
                    .padding(16.0)
                    .margin_vertical(16.0)
                    .background_brush(Brush::Linear {
                        start: Vec2 { x: 0.0, y: 0.0 },
                        end: Vec2 { x: 1.0, y: 0.0 },
                        start_color: Color::from_hex("#7F1D1D"),
                        end_color: Color::from_hex("#450A0A"),
                    })
                    .border(1.0, Color::from_hex("#DC2626"), 16.0)
                    .clip_rounded(16.0))
                .child((
                    Text("⚠️").size(20.0).modifier(Modifier::new().padding(4.0)),
                    Space(Modifier::new().width(12.0)),
                    Text(err)
                        .color(Color::from_hex("#FCA5A5"))
                        .size(14.0)
                        .modifier(Modifier::new().flex_grow(1.0)),
                    secondary_button("Dismiss", {
                        let store = store.clone();
                        move || store.dispatch(Action::ClearError)
                    }),
                ))
            } else {
                Box(Modifier::new())
            },
            // Header bar
            Row(Modifier::new().padding_values(PaddingValues {
                left: 8.0,
                right: 8.0,
                top: 8.0,
                bottom: 16.0,
            }))
            .child((
                Text("soredowe")
                    .size(28.0)
                    .color(Color::from_hex("#F9FAFB"))
                    .modifier(Modifier::new().padding(8.0)),
                Text("Package Manager")
                    .size(14.0)
                    .color(Color::from_hex("#6B7280"))
                    .modifier(Modifier::new().padding(12.0)),
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
            )),
            separator(),
            // Search row
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
                        .background_brush(Brush::Linear {
                            start: Vec2 { x: 0.0, y: 0.0 },
                            end: Vec2 { x: 0.0, y: 1.0 },
                            start_color: Color::from_hex("#1F2937"),
                            end_color: Color::from_hex("#111827"),
                        })
                        .border(1.0, Color::from_hex("#374151"), 14.0)
                        .clip_rounded(14.0)
                        .semantics(Semantics {
                            role: Role::TextField,
                            label: Some("Search field".into()),
                            focused: false,
                            enabled: true,
                        }),
                    Some({
                        let store = store.clone();
                        move |text: String| {
                            store.dispatch(Action::SetQuery(text));
                        }
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
                    move || {
                        store.dispatch(Action::Search);
                    }
                }),
                Space(Modifier::new().width(24.0)),
                // Filters
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
                // Sort buttons
                Text("Sort:")
                    .size(13.0)
                    .color(Color::from_hex("#6B7280"))
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
            ]),
            Space(Modifier::new().height(16.0)),
            // Main content grid
            {
                let left_span = 4;
                let right_span = 2;

                Grid(
                    6,
                    Modifier::new().fill_max_size().padding(8.0),
                    vec![
                        // Left: result list
                        Column(Modifier::new().grid_span(left_span, 1).padding(8.0)).child(
                            if s.results.is_empty() {
                                Column(
                                    Modifier::new()
                                        .padding(48.0)
                                        .fill_max_width()
                                        .background_brush(Brush::Linear {
                                            start: Vec2 { x: 0.0, y: 0.0 },
                                            end: Vec2 { x: 1.0, y: 1.0 },
                                            start_color: Color::from_hex("#1F2937"),
                                            end_color: Color::from_hex("#111827"),
                                        })
                                        .clip_rounded(20.0)
                                        .border(1.0, Color::from_hex("#374151"), 20.0),
                                )
                                .child((
                                    Text("🔍").size(48.0).modifier(
                                        Modifier::new().align_self_center().padding(16.0),
                                    ),
                                    Text("No results")
                                        .size(18.0)
                                        .color(Color::from_hex("#9CA3AF"))
                                        .modifier(Modifier::new().align_self_center()),
                                    Text("Try searching for a package")
                                        .size(14.0)
                                        .color(Color::from_hex("#6B7280"))
                                        .modifier(Modifier::new().align_self_center().padding(8.0)),
                                ))
                            } else {
                                LazyColumn(
                                    s.results.clone(),
                                    72.0, // Taller rows for more breathing room
                                    remember_with_key("scroll", || LazyColumnState::new()),
                                    Modifier::new().fill_max_width().height(650.0),
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
                        Column(Modifier::new().grid_span(right_span, 1).padding(8.0))
                            .child(details_card(store.clone())),
                    ],
                    12.0,
                    16.0,
                )
            },
            // Footer / status bar
            Row(Modifier::new()
                .padding(16.0)
                .margin_vertical(8.0)
                .background_brush(Brush::Linear {
                    start: Vec2 { x: 0.0, y: 0.0 },
                    end: Vec2 { x: 1.0, y: 0.0 },
                    start_color: Color::from_hex("#1F2937"),
                    end_color: Color::from_hex("#111827"),
                })
                .clip_rounded(14.0)
                .border(1.0, Color::from_hex("#374151"), 14.0))
            .child((
                Text("●")
                    .size(10.0)
                    .color(Color::from_hex("#22C55E"))
                    .modifier(Modifier::new().padding(4.0)),
                Text("Status")
                    .size(13.0)
                    .color(Color::from_hex("#9CA3AF"))
                    .modifier(Modifier::new().padding(4.0)),
                Text(format!(
                    "  {}",
                    s.progress_log.lines().last().unwrap_or("Ready")
                ))
                .size(13.0)
                .color(Color::from_hex("#D1D5DB"))
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
            )),
            if s.log_expanded {
                Box(Modifier::new()
                    .fill_max_width()
                    .height(200.0)
                    .margin(12.0)
                    .padding(16.0)
                    .background_brush(Brush::Linear {
                        start: Vec2 { x: 0.0, y: 0.0 },
                        end: Vec2 { x: 0.0, y: 1.0 },
                        start_color: Color::from_hex("#111827"),
                        end_color: Color::from_hex("#0F172A"),
                    })
                    .clip_rounded(14.0)
                    .border(1.0, Color::from_hex("#374151"), 14.0))
                .child(
                    Text(s.progress_log.clone())
                        .size(12.0)
                        .color(Color::from_hex("#A5B4FC")),
                )
            } else {
                Box(Modifier::new())
            },
        )),
    )
}
