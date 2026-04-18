//! Reusable, self-contained widgets.

use crate::theme::*;
use repose_core::*;
use repose_ui::*;

pub fn badge(text: &str, start_color: &str, end_color: &str) -> View {
    Text(text.to_string())
        .color(Color::from_hex(TEXT_PRIMARY))
        .size(BADGE_FONT)
        .modifier(
            Modifier::new()
                .padding_values(PaddingValues {
                    left: 10.0,
                    right: 10.0,
                    top: 4.0,
                    bottom: 4.0,
                })
                .background_brush(diag_gradient(start_color, end_color))
                .clip_rounded(CORNER_SM),
        )
}

pub fn aur_badge() -> View {
    badge("AUR", PURPLE_BADGE_START, PURPLE_BADGE_END)
}
pub fn repo_badge() -> View {
    badge("Repo", TEAL_BADGE_START, TEAL_BADGE_END)
}
pub fn installed_badge() -> View {
    badge("Installed", INDIGO_BADGE_START, INDIGO_BADGE_END)
}

pub fn source_badge(is_aur: bool) -> View {
    if is_aur {
        aur_badge()
    } else {
        repo_badge()
    }
}

pub fn chip(label: &str, on: bool, on_toggle: impl Fn() + 'static) -> View {
    let (start, end, border) = if on {
        (CHIP_ON_START, CHIP_ON_END, CHIP_ON_BORDER)
    } else {
        (CHIP_OFF_START, CHIP_OFF_END, CHIP_OFF_BORDER)
    };

    Button(Text(label).size(SMALL_FONT), on_toggle).modifier(
        Modifier::new()
            .padding_values(PaddingValues {
                left: 16.0,
                right: 16.0,
                top: 8.0,
                bottom: 8.0,
            })
            .background_brush(v_gradient(start, end))
            .clip_rounded(CORNER_XL)
            .border(1.0, Color::from_hex(border), CORNER_XL),
    )
}

pub fn styled_button(label: &str, on_click: impl Fn() + 'static) -> View {
    Button(Text(label).size(BODY_FONT), on_click).modifier(pill_modifier(
        BLUE_START,
        BLUE_END,
        BLUE_BORDER,
    ))
}

pub fn secondary_button(label: &str, on_click: impl Fn() + 'static) -> View {
    Button(
        Text(label)
            .size(BODY_FONT)
            .color(Color::from_hex(TEXT_SECONDARY)),
        on_click,
    )
    .modifier(pill_modifier(CHIP_OFF_START, CHIP_OFF_END, CHIP_OFF_BORDER))
}

pub fn success_button(label: &str, on_click: impl Fn() + 'static) -> View {
    Button(Text(label).size(BODY_FONT), on_click).modifier(action_pill(
        GREEN_START,
        GREEN_END,
        GREEN_BORDER,
    ))
}

pub fn danger_button(label: &str, on_click: impl Fn() + 'static) -> View {
    Button(Text(label).size(BODY_FONT), on_click)
        .modifier(action_pill(RED_START, RED_END, RED_BORDER))
}

pub fn separator() -> View {
    Box(Modifier::new()
        .height(1.0)
        .fill_max_width()
        .background_brush(h_gradient(CARD_END, CARD_BORDER))
        .margin(8.0))
}

pub fn divider() -> View {
    Box(Modifier::new()
        .height(1.0)
        .fill_max_width()
        .background(Color::from_hex(CARD_BORDER))
        .margin_vertical(16.0))
}

pub fn empty_state(emoji: &str, title: &str, subtitle: &str) -> View {
    Column(
        Modifier::new()
            .padding(48.0)
            .fill_max_width()
            .then(card_modifier()),
    )
    .child((
        Text(emoji)
            .size(EMOJI_FONT)
            .modifier(Modifier::new().align_self_center().padding(16.0)),
        Text(title)
            .size(18.0)
            .color(Color::from_hex(TEXT_MUTED))
            .modifier(Modifier::new().align_self_center()),
        Text(subtitle)
            .size(BODY_FONT)
            .color(Color::from_hex(TEXT_DIMMED))
            .modifier(Modifier::new().align_self_center().padding(8.0)),
    ))
}
