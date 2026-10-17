//! Global style tweaks and cell colors for quality values.

use eframe::egui::{self, Color32};

/// Always-visible, wider scroll bars instead of egui's thin floating ones that
/// only appear on hover; long ticket lists need an obvious scroll position.
pub fn apply(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.spacing.scroll = egui::style::ScrollStyle {
            bar_width: 10.0,
            // Thousands of rows would otherwise shrink the handle to a sliver.
            handle_min_length: 40.0,
            foreground_color: true,
            ..egui::style::ScrollStyle::solid()
        };
    });
}

const GOOD: Color32 = Color32::from_rgb(120, 200, 120);
const FAIR: Color32 = Color32::from_rgb(230, 210, 100);
const POOR: Color32 = Color32::from_rgb(240, 160, 80);
const BAD: Color32 = Color32::from_rgb(230, 100, 100);

/// Same bands as `ipview stats`.
pub fn mos_color(mos: f64) -> Color32 {
    if mos >= 4.0 {
        GOOD
    } else if mos >= 3.6 {
        FAIR
    } else if mos >= 3.1 {
        POOR
    } else {
        BAD
    }
}

pub fn loss_color(pct: f64) -> Option<Color32> {
    if pct >= 5.0 {
        Some(BAD)
    } else if pct >= 1.0 {
        Some(POOR)
    } else if pct > 0.0 {
        Some(FAIR)
    } else {
        None
    }
}
