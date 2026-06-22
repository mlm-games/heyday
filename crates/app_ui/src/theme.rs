use repose_core::*;

pub const R_SM: f32 = 8.0;
pub const R_MD: f32 = 12.0;
pub const R_LG: f32 = 16.0;

pub fn setup_theme() {
    set_theme_default(Theme {
        colors: ColorScheme {
            primary: Color::from_hex("#3B82F6"),
            on_primary: Color::from_hex("#FFFFFF"),
            primary_container: Color::from_hex("#172554"),
            on_primary_container: Color::from_hex("#D1D5DB"),

            secondary: Color::from_hex("#818CF8"),
            on_secondary: Color::from_hex("#FFFFFF"),
            secondary_container: Color::from_hex("#1E1B4B"),
            on_secondary_container: Color::from_hex("#C7D2FE"),

            tertiary: Color::from_hex("#A78BFA"),
            on_tertiary: Color::from_hex("#FFFFFF"),
            tertiary_container: Color::from_hex("#1E1045"),
            on_tertiary_container: Color::from_hex("#DDD6FE"),

            error: Color::from_hex("#F87171"),
            on_error: Color::from_hex("#FFFFFF"),
            error_container: Color::from_hex("#2B0606"),
            on_error_container: Color::from_hex("#FECACA"),

            background: Color::from_hex("#0F172A"),
            on_background: Color::from_hex("#F9FAFB"),
            surface: Color::from_hex("#111827"),
            on_surface: Color::from_hex("#F9FAFB"),
            surface_variant: Color::from_hex("#1F2937"),
            on_surface_variant: Color::from_hex("#9CA3AF"),
            surface_container_lowest: Color::from_hex("#020617"),
            surface_container_low: Color::from_hex("#111827"),
            surface_container: Color::from_hex("#1A2332"),
            surface_container_high: Color::from_hex("#1F2937"),
            surface_container_highest: Color::from_hex("#2A3040"),
            surface_bright: Color::from_hex("#1E293B"),
            surface_dim: Color::from_hex("#0F172A"),
            surface_tint: Color::from_hex("#3B82F6"),

            inverse_surface: Color::from_hex("#F9FAFB"),
            inverse_on_surface: Color::from_hex("#111827"),
            inverse_primary: Color::from_hex("#2563EB"),

            outline: Color::from_hex("#374151"),
            outline_variant: Color::from_hex("#1F2937"),

            scrim: Color::from_hex("#000000"),
            shadow: Color::from_hex("#000000"),
            focus: Color::from_hex("#3B82F6"),
        },
        ..Default::default()
    });
}
