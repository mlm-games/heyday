//! Each function here should only return a `View` and take only the data it needs.
//! No function here knows about `Store` or `Action`.

use crate::theme::*;
use bigcolor::BigColor;
use colorhash::ColorHash;
use domain::{PackageSummary, Source};
use repose_core::*;
use repose_material::material3::Button;
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
pub fn flatpak_badge() -> View {
    badge("Flatpak", "#3584e4", "#1a1a2e")
}
pub fn appimage_badge() -> View {
    badge("AppImage", "#f9a03c", "#2e1a0a")
}
pub fn installed_badge() -> View {
    badge("Installed", INDIGO, INDIGO_BG)
}
fn repo_colors(repo: &str) -> (String, String) {
    if let Some(c) = match repo {
        "core" => Some(("#FCD34D", "#451A03")),
        "extra" => Some(("#60A5FA", "#1E3A5F")),
        "community" => Some(("#34D399", "#064E3B")),
        "multilib" => Some(("#F472B6", "#4A0E4E")),
        "testing" => Some(("#FBBF24", "#422006")),
        _ => None,
    } {
        return (c.0.into(), c.1.into());
    }

    let rgb = ColorHash::new().lightness(0.14).saturation(0.55).rgb(repo);
    let bg = BigColor::from_rgb(rgb.red() as u8, rgb.green() as u8, rgb.blue() as u8, 1.0);
    let fg = bg.get_contrast_color(1.0);
    (fg.to_hex_string(false), bg.to_hex_string(false))
}

pub fn source_badge(pkg: &PackageSummary) -> View {
    match pkg.id.source {
        Source::Aur => aur_badge(),
        Source::Flatpak => flatpak_badge(),
        Source::AppImage => appimage_badge(),
        Source::Repo => {
            if let Some(repo) = &pkg.id.repo {
                let (fg, bg) = repo_colors(repo);
                badge(repo, &fg, &bg)
            } else {
                repo_badge()
            }
        }
    }
}

pub fn chip(label: &str, on: bool, on_click: impl Fn() + 'static) -> View {
    let (bg, border, fg) = if on {
        (CHIP_ON_BG, CHIP_ON_BORDER, CHIP_ON_TEXT)
    } else {
        (CHIP_OFF_BG, CHIP_OFF_BORDER, CHIP_OFF_TEXT)
    };
    Button(
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
        on_click,
        || Text(label).size(FONT_SM).color(Color::from_hex(fg)),
    )
}

pub fn primary_button(label: &str, on_click: impl Fn() + 'static) -> View {
    Button(pill(BLUE_BG, BLUE_BORDER), on_click, || {
        Text(label)
            .size(FONT_BASE)
            .color(Color::from_hex(TEXT_PRIMARY))
    })
}

pub fn secondary_button(label: &str, on_click: impl Fn() + 'static) -> View {
    Button(pill(CHIP_OFF_BG, CHIP_OFF_BORDER), on_click, || {
        Text(label)
            .size(FONT_BASE)
            .color(Color::from_hex(TEXT_MUTED))
    })
}

pub fn success_button(label: &str, on_click: impl Fn() + 'static) -> View {
    Button(pill(GREEN_BG, GREEN_BORDER), on_click, || {
        Text(label)
            .size(FONT_BASE)
            .color(Color::from_hex(TEXT_PRIMARY))
    })
}

pub fn danger_button(label: &str, on_click: impl Fn() + 'static) -> View {
    Button(pill(RED_BG, RED_BORDER), on_click, || {
        Text(label).size(FONT_BASE).color(Color::from_hex(RED))
    })
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
            .max_lines(3)
            .overflow_ellipsize()
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
        Row(Modifier::new().fill_max_width().flex_wrap(FlexWrap::Wrap)).child(
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

pub fn pkg_avatar(name: &str, size: f32) -> View {
    let (fg, bg) = {
        let rgb = ColorHash::new().lightness(0.18).saturation(0.55).rgb(name);
        let bg = BigColor::from_rgb(rgb.red() as u8, rgb.green() as u8, rgb.blue() as u8, 1.0);
        let fg = bg.get_contrast_color(1.0);
        (fg.to_hex_string(false), bg.to_hex_string(false))
    };
    let letter = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().next().unwrap_or(c))
        .unwrap_or('?');

    Column(
        Modifier::new()
            .width(size)
            .height(size)
            .background(Color::from_hex(&bg))
            .clip_rounded(size / 2.0)
            .justify_content(JustifyContent::Center)
            .align_items(AlignItems::Center),
    )
    .child(
        Text(letter.to_string())
            .size(size * 0.48)
            .color(Color::from_hex(&fg)),
    )
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
