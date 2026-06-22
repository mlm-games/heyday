use crate::theme::*;
use repose_core::*;
use repose_material::material3::*;
use repose_ui::*;


pub fn primary_button(label: &str, on_click: impl Fn() + 'static) -> View {
    let label = label.to_string();
    FilledButton(Modifier::new(), on_click, ButtonConfig::default(), move || {
        Text(label.clone()).size(14.0)
    })
}

pub fn outline_button(label: &str, on_click: impl Fn() + 'static) -> View {
    let label = label.to_string();
    OutlinedButton(Modifier::new(), on_click, ButtonConfig::default(), move || {
        Text(label.clone()).size(14.0)
    })
}

pub fn tonal_button(label: &str, on_click: impl Fn() + 'static) -> View {
    let label = label.to_string();
    FilledTonalButton(Modifier::new(), on_click, ButtonConfig::default(), move || {
        Text(label.clone()).size(14.0)
    })
}

pub fn text_button(label: &str, on_click: impl Fn() + 'static) -> View {
    let label = label.to_string();
    TextButton(Modifier::new(), on_click, ButtonConfig::default(), move || {
        Text(label.clone()).size(14.0)
    })
}

pub fn danger_button(label: &str, on_click: impl Fn() + 'static) -> View {
    let th = theme();
    let label = label.to_string();
    FilledTonalButton(
        Modifier::new(),
        on_click,
        ButtonConfig {
            container_color: Some(th.error_container),
            content_color: Some(th.on_error_container),
            ..Default::default()
        },
        move || Text(label.clone()).size(14.0),
    )
}

pub fn success_button(label: &str, on_click: impl Fn() + 'static) -> View {
    let th = theme();
    let label = label.to_string();
    FilledButton(
        Modifier::new(),
        on_click,
        ButtonConfig {
            container_color: Some(th.primary),
            content_color: Some(th.on_primary),
            ..Default::default()
        },
        move || Text(label.clone()).size(14.0),
    )
}

pub fn chip(label: &str, on: bool, on_click: impl Fn() + 'static) -> View {
    let label = label.to_string();
    FilterChip(
        on,
        on_click,
        Text(label.clone()).size(13.0),
        None,
        None,
        ChipConfig::default(),
    )
}

pub fn source_badge(is_aur: bool) -> View {
    if is_aur {
        aur_badge()
    } else {
        repo_badge()
    }
}

pub fn aur_badge() -> View {
    Badge(Some("AUR"), BadgeConfig {
        color: theme().tertiary_container,
        label_color: theme().on_tertiary_container,
        ..Default::default()
    })
}

pub fn repo_badge() -> View {
    Badge(Some("Repo"), BadgeConfig {
        color: theme().secondary_container,
        label_color: theme().on_secondary_container,
        ..Default::default()
    })
}

pub fn installed_badge() -> View {
    Badge(Some("Installed"), BadgeConfig {
        color: theme().primary_container,
        label_color: theme().on_primary_container,
        ..Default::default()
    })
}

pub fn empty_state(title: &str, subtitle: &str) -> View {
    let th = theme();
    Column(
        Modifier::new()
            .padding(32.0)
            .fill_max_width()
            .background(th.surface_container_low)
            .border(1.0, th.outline_variant, R_LG)
            .clip_rounded(R_LG),
    )
    .child((
        Text(title)
            .size(16.0)
            .color(th.on_surface_variant)
            .modifier(Modifier::new().align_self_center()),
        Space(Modifier::new().height(6.0)),
        Text(subtitle)
            .size(13.0)
            .color(th.on_surface_variant)
            .modifier(Modifier::new().align_self_center()),
    ))
}

pub fn detail_row(label: &str, value: &str) -> View {
    if value.is_empty() {
        return Box(Modifier::new());
    }
    let th = theme();
    Row(Modifier::new().padding_values(PaddingValues {
        left: 0.0,
        right: 0.0,
        top: 4.0,
        bottom: 4.0,
    }))
    .child((
        Text(label.to_string())
            .size(13.0)
            .color(th.on_surface_variant)
            .modifier(Modifier::new().width(110.0)),
        Text(value.to_string())
            .size(13.0)
            .color(th.on_surface)
            .max_lines(3)
            .overflow_ellipsize()
            .modifier(Modifier::new().flex_grow(1.0)),
    ))
}

pub fn tag_list(label: &str, items: &[String]) -> View {
    if items.is_empty() {
        return Box(Modifier::new());
    }
    let th = theme();
    Column(Modifier::new().padding_values(PaddingValues {
        left: 0.0,
        right: 0.0,
        top: 8.0,
        bottom: 4.0,
    }))
    .child((
        Text(label.to_string())
            .size(13.0)
            .color(th.on_surface_variant)
            .modifier(Modifier::new().padding_values(PaddingValues {
                left: 0.0,
                right: 0.0,
                top: 0.0,
                bottom: 6.0,
            })),
        Row(Modifier::new().fill_max_width().flex_wrap(FlexWrap::Wrap)).child(
            items
                .iter()
                .take(30)
                .map(|dep| {
                    Text(dep.clone())
                        .size(11.0)
                        .color(th.on_surface)
                        .modifier(
                            Modifier::new()
                                .padding_values(PaddingValues {
                                    left: 8.0,
                                    right: 8.0,
                                    top: 3.0,
                                    bottom: 3.0,
                                })
                                .margin(2.0)
                                .background(th.surface_container_high)
                                .clip_rounded(R_SM)
                                .border(1.0, th.outline_variant, R_SM),
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
