use repose_core::*;

pub const BG_START: &str = "#0F172A";
pub const BG_END: &str = "#020617";

pub const CARD_BG: &str = "#111827";
pub const CARD_BORDER: &str = "#1F2937";
pub const CARD_SURFACE: &str = "#1A2332";

pub const AUR_BG: &str = "#14112E";
pub const AUR_BORDER: &str = "#4338CA";

pub const FLATPAK_BG: &str = "#0E1628";
pub const FLATPAK_BORDER: &str = "#3584e4";

pub const APPIMAGE_BG: &str = "#1C0F08";
pub const APPIMAGE_BORDER: &str = "#f9a03c";

pub const SEL_BG: &str = "#172554";
pub const SEL_BORDER: &str = "#3B82F6";

pub const TEXT_PRIMARY: &str = "#F9FAFB";
pub const TEXT_SECONDARY: &str = "#D1D5DB";
pub const TEXT_MUTED: &str = "#9CA3AF";
pub const TEXT_DIMMED: &str = "#6B7280";

pub const GREEN: &str = "#22C55E";
pub const GREEN_BG: &str = "#052E16";
pub const GREEN_BORDER: &str = "#16A34A";

pub const RED: &str = "#F87171";
pub const RED_BG: &str = "#2B0606";
pub const RED_BORDER: &str = "#DC2626";

pub const BLUE_BORDER: &str = "#2563EB";

pub const PURPLE: &str = "#A78BFA";
pub const PURPLE_BG: &str = "#1E1045";

pub const TEAL: &str = "#2DD4BF";
pub const TEAL_BG: &str = "#042F2E";

pub const INDIGO: &str = "#818CF8";
pub const INDIGO_BG: &str = "#1E1B4B";

pub const LOG_TEXT: &str = "#A5B4FC";
pub const STATUS_DOT: &str = "#22C55E";

pub const R_SM: f32 = 8.0;
pub const R_MD: f32 = 12.0;
pub const R_LG: f32 = 16.0;

pub const FONT_XS: f32 = 11.0;
pub const FONT_SM: f32 = 13.0;
pub const FONT_BASE: f32 = 14.0;
pub const FONT_LG: f32 = 16.0;
pub const FONT_XL: f32 = 20.0;
pub const FONT_2XL: f32 = 24.0;

pub fn v_gradient(top: &str, bot: &str) -> Brush {
    Brush::Linear {
        start: Vec2 { x: 0.0, y: 0.0 },
        end: Vec2 { x: 0.0, y: 1.0 },
        start_color: Color::from_hex(top),
        end_color: Color::from_hex(bot),
    }
}
