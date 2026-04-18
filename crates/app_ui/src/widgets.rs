//! Each function here should only return a `View` and take only the data it needs.
//! No function here knows about `Store` or `Action`.

use crate::theme::*;
use repose_core::*;
use repose_ui::*;

fn badge(label: &str, fg: &str, bg: &str) -> View {
    Text(label.to_string())
        .size(FONT_XS)
        .color(Color::from_hex(fg))
        .modifier(
            Modifier::new()
                .padding_values(PaddingValues {
                    left: 8.0,
                    right: 8.0,
                    top: 3.0,
                    bottom: 3.0,
                })
                .background(Color::from_hex(bg))
                .clip_rounded(R_SM),
        )
}

pub fn aur_badge() -> View {
    badge("AUR", PURPLE, PURPLE_BG)
}
pub fn repo_badge() -> View {
    badge("Repo", TEAL, TEAL_BG)
}
pub fn installed_badge() -> View {
    badge("Installed", INDIGO, INDIGO_BG)
}
pub fn source_badge(is_aur: bool) -> View {
    if is_aur { aur_badge() } else { repo_badge() }
}

pub fn chip(label: &str, on: bool, on_click: impl Fn() + 'static) -> View {
    let (bg, border, fg) = if on {
        (CHIP_ON_BG, CHIP_ON_BORDER, CHIP_ON_TEXT)
    } else {
        (CHIP_OFF_BG, CHIP_OFF_BORDER, CHIP_OFF_TEXT)
    };
    Button(
        Text(label).size(FONT_SM).color(Color::from_hex(fg)),
        on_click,
    )
    .modifier(
        Modifier::new()
            .padding_values(PaddingValues {
                left: 14.0,
                right: 14.0,
                top: 6.0,
                bottom: 6.0,
            })
            .background(Color::from_hex(bg))
            .border(1.0, Color::from_hex(border), 999.0)
            .clip_rounded(999.0),
    )
}

pub fn primary_button(label: &str, on_click: impl Fn() + 'static) -> View {
    Button(
        Text(label)
            .size(FONT_BASE)
            .color(Color::from_hex(TEXT_PRIMARY)),
        on_click,
    )
    .modifier(pill(BLUE_BG, BLUE_BORDER))
}

pub fn secondary_button(label: &str, on_click: impl Fn() + 'static) -> View {
    Button(
        Text(label)
            .size(FONT_BASE)
            .color(Color::from_hex(TEXT_MUTED)),
        on_click,
    )
    .modifier(pill(CHIP_OFF_BG, CHIP_OFF_BORDER))
}

pub fn success_button(label: &str, on_click: impl Fn() + 'static) -> View {
    Button(
        Text(label)
            .size(FONT_BASE)
            .color(Color::from_hex(TEXT_PRIMARY)),
        on_click,
    )
    .modifier(pill(GREEN_BG, GREEN_BORDER))
}

pub fn danger_button(label: &str, on_click: impl Fn() + 'static) -> View {
    Button(
        Text(label).size(FONT_BASE).color(Color::from_hex(RED)),
        on_click,
    )
    .modifier(pill(RED_BG, RED_BORDER))
}

pub fn divider() -> View {
    Box(Modifier::new()
        .height(1.0)
        .fill_max_width()
        .background(Color::from_hex(CARD_BORDER))
        .margin_vertical(4.0))
}

pub fn empty_state(title: &str, subtitle: &str) -> View {
    Column(
        Modifier::new()
            .padding(32.0)
            .fill_max_width()
            .then(card_mod()),
    )
    .child((
        Text(title)
            .size(FONT_LG)
            .color(Color::from_hex(TEXT_MUTED))
            .modifier(Modifier::new().align_self_center()),
        Space(Modifier::new().height(6.0)),
        Text(subtitle)
            .size(FONT_SM)
            .color(Color::from_hex(TEXT_DIMMED))
            .modifier(Modifier::new().align_self_center()),
    ))
}

/// Labelled metadata row:  "Label    value"
pub fn detail_row(label: &str, value: &str) -> View {
    if value.is_empty() {
        return Box(Modifier::new());
    }
    Row(Modifier::new().padding_values(PaddingValues {
        left: 0.0,
        right: 0.0,
        top: 4.0,
        bottom: 4.0,
    }))
    .child((
        Text(label.to_string())
            .size(FONT_SM)
            .color(Color::from_hex(TEXT_DIMMED))
            .modifier(Modifier::new().width(110.0)),
        Text(value.to_string())
            .size(FONT_SM)
            .color(Color::from_hex(TEXT_SECONDARY))
            .modifier(Modifier::new().flex_grow(1.0)),
    ))
}

/// Renders a tag list (e.g. dependencies) as a flowing set of pills.
pub fn tag_list(label: &str, items: &[String]) -> View {
    if items.is_empty() {
        return Box(Modifier::new());
    }
    Column(Modifier::new().padding_values(PaddingValues {
        left: 0.0,
        right: 0.0,
        top: 8.0,
        bottom: 4.0,
    }))
    .child((
        Text(label.to_string())
            .size(FONT_SM)
            .color(Color::from_hex(TEXT_DIMMED))
            .modifier(Modifier::new().padding_values(PaddingValues {
                left: 0.0,
                right: 0.0,
                top: 0.0,
                bottom: 6.0,
            })),
        // Simple wrapping: show them in rows. LazyColumn not needed for ≤30 deps.
        Row(Modifier::new().fill_max_width()).child(
            items
                .iter()
                .take(30)
                .map(|dep| {
                    Text(dep.clone())
                        .size(FONT_XS)
                        .color(Color::from_hex(TEXT_SECONDARY))
                        .modifier(
                            Modifier::new()
                                .padding_values(PaddingValues {
                                    left: 8.0,
                                    right: 8.0,
                                    top: 3.0,
                                    bottom: 3.0,
                                })
                                .margin(2.0)
                                .background(Color::from_hex(CARD_SURFACE))
                                .clip_rounded(R_SM)
                                .border(1.0, Color::from_hex(CARD_BORDER), R_SM),
                        )
                })
                .collect::<Vec<_>>(),
        ),
    ))
}

pub fn format_bytes(b: u64) -> String {
    if b >= 1024 * 1024 {
        format!("{:.1} MiB", b as f64 / (1024.0 * 1024.0))
    } else if b >= 1024 {
        format!("{:.0} KiB", b as f64 / 1024.0)
    } else {
        format!("{b} B")
    }
}
