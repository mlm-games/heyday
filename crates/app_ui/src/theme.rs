use repose_core::*;

pub const BG_START: &str = "#0F172A";
pub const BG_END: &str = "#050B18";

pub const R_SM: f32 = 8.0;
pub const R_MD: f32 = 12.0;
pub const R_LG: f32 = 16.0;

pub fn v_gradient(top: &str, bot: &str) -> Brush {
    Brush::Linear {
        start: Vec2 { x: 0.0, y: 0.0 },
        end: Vec2 { x: 0.0, y: 1.0 },
        start_color: Color::from_hex(top),
        end_color: Color::from_hex(bot),
    }
}

pub fn setup_theme() {
    set_theme_default(Theme {
        colors: ColorScheme {
            primary: Color::from_hex("#3B82F6"),
            on_primary: Color::from_hex("#FFFFFF"),
            primary_container: Color::from_hex("#1E3A8A"),
            on_primary_container: Color::from_hex("#BFDBFE"),

            secondary: Color::from_hex("#818CF8"),
            on_secondary: Color::from_hex("#FFFFFF"),
            secondary_container: Color::from_hex("#312E81"),
            on_secondary_container: Color::from_hex("#C7D2FE"),

            tertiary: Color::from_hex("#A78BFA"),
            on_tertiary: Color::from_hex("#FFFFFF"),
            tertiary_container: Color::from_hex("#2E1065"),
            on_tertiary_container: Color::from_hex("#DDD6FE"),

            error: Color::from_hex("#F87171"),
            on_error: Color::from_hex("#FFFFFF"),
            error_container: Color::from_hex("#450A0A"),
            on_error_container: Color::from_hex("#FECACA"),

            background: Color::from_hex("#0F172A"),
            on_background: Color::from_hex("#F9FAFB"),
            surface: Color::from_hex("#111827"),
            on_surface: Color::from_hex("#F9FAFB"),
            surface_variant: Color::from_hex("#1F2937"),
            on_surface_variant: Color::from_hex("#9CA3AF"),
            surface_container_lowest: Color::from_hex("#020617"),
            surface_container_low: Color::from_hex("#0F172A"),
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
