use repose_core::*;

pub const BG_START: &str = "#0F172A";
pub const BG_END: &str = "#020617";

pub const CARD_START: &str = "#1F2937";
pub const CARD_END: &str = "#111827";
pub const CARD_BORDER: &str = "#374151";

pub const AUR_START: &str = "#1E1B4B";
pub const AUR_END: &str = "#0F0D24";
pub const AUR_BORDER: &str = "#4338CA";

pub const SELECTED_START: &str = "#1E40AF";
pub const SELECTED_END: &str = "#1E3A8A";
pub const SELECTED_BORDER: &str = "#3B82F6";

pub const TEXT_PRIMARY: &str = "#F9FAFB";
pub const TEXT_SECONDARY: &str = "#D1D5DB";
pub const TEXT_MUTED: &str = "#9CA3AF";
pub const TEXT_DIMMED: &str = "#6B7280";

pub const GREEN_START: &str = "#22C55E";
pub const GREEN_END: &str = "#16A34A";
pub const GREEN_BORDER: &str = "#34D399";

pub const RED_START: &str = "#EF4444";
pub const RED_END: &str = "#DC2626";
pub const RED_BORDER: &str = "#F87171";

pub const BLUE_START: &str = "#3B82F6";
pub const BLUE_END: &str = "#2563EB";
pub const BLUE_BORDER: &str = "#60A5FA";

pub const PURPLE_BADGE_START: &str = "#7C3AED";
pub const PURPLE_BADGE_END: &str = "#5B21B6";

pub const TEAL_BADGE_START: &str = "#059669";
pub const TEAL_BADGE_END: &str = "#047857";

pub const INDIGO_BADGE_START: &str = "#6366F1";
pub const INDIGO_BADGE_END: &str = "#4F46E5";

pub const CHIP_ON_START: &str = "#22C55E";
pub const CHIP_ON_END: &str = "#16A34A";
pub const CHIP_ON_BORDER: &str = "#34D399";
pub const CHIP_OFF_START: &str = "#374151";
pub const CHIP_OFF_END: &str = "#1F2937";
pub const CHIP_OFF_BORDER: &str = "#4B5563";

pub const ERROR_BG_START: &str = "#7F1D1D";
pub const ERROR_BG_END: &str = "#450A0A";
pub const ERROR_BORDER: &str = "#DC2626";
pub const ERROR_TEXT: &str = "#FCA5A5";

pub const LOG_TEXT: &str = "#A5B4FC";
pub const STATUS_DOT: &str = "#22C55E";

pub const CORNER_SM: f32 = 12.0;
pub const CORNER_MD: f32 = 14.0;
pub const CORNER_LG: f32 = 16.0;
pub const CORNER_XL: f32 = 20.0;

pub const BADGE_FONT: f32 = 11.0;
pub const BODY_FONT: f32 = 14.0;
pub const SMALL_FONT: f32 = 13.0;
pub const TITLE_FONT: f32 = 22.0;
pub const HEADER_FONT: f32 = 28.0;
pub const PKG_NAME_FONT: f32 = 16.0;
pub const EMOJI_FONT: f32 = 48.0;

/// Vertical linear gradient (most common case).
pub fn v_gradient(start: &str, end: &str) -> Brush {
    Brush::Linear {
        start: Vec2 { x: 0.0, y: 0.0 },
        end: Vec2 { x: 0.0, y: 1.0 },
        start_color: Color::from_hex(start),
        end_color: Color::from_hex(end),
    }
}

/// Diagonal gradient (top-left → bottom-right).
pub fn diag_gradient(start: &str, end: &str) -> Brush {
    Brush::Linear {
        start: Vec2 { x: 0.0, y: 0.0 },
        end: Vec2 { x: 1.0, y: 1.0 },
        start_color: Color::from_hex(start),
        end_color: Color::from_hex(end),
    }
}

/// Horizontal gradient.
pub fn h_gradient(start: &str, end: &str) -> Brush {
    Brush::Linear {
        start: Vec2 { x: 0.0, y: 0.0 },
        end: Vec2 { x: 1.0, y: 0.0 },
        start_color: Color::from_hex(start),
        end_color: Color::from_hex(end),
    }
}

/// Standard card modifier with diagonal gradient + border + rounded clip.
pub fn card_modifier() -> Modifier {
    Modifier::new()
        .background_brush(diag_gradient(CARD_START, CARD_END))
        .border(1.0, Color::from_hex(CARD_BORDER), CORNER_XL)
        .clip_rounded(CORNER_XL)
}

/// Standard pill-button modifier.
pub fn pill_modifier(start: &str, end: &str, border: &str) -> Modifier {
    Modifier::new()
        .padding_values(PaddingValues {
            left: 24.0,
            right: 24.0,
            top: 10.0,
            bottom: 10.0,
        })
        .background_brush(v_gradient(start, end))
        .clip_rounded(CORNER_MD)
        .border(1.0, Color::from_hex(border), CORNER_MD)
}

/// Smaller pill for action buttons on cards.
pub fn action_pill(start: &str, end: &str, border: &str) -> Modifier {
    Modifier::new()
        .padding_values(PaddingValues {
            left: 20.0,
            right: 20.0,
            top: 10.0,
            bottom: 10.0,
        })
        .background_brush(v_gradient(start, end))
        .clip_rounded(CORNER_MD)
        .border(1.0, Color::from_hex(border), CORNER_MD)
}
