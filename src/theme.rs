//! The program's look, shared by the desktop and Android builds: fonts (Inter and
//! JetBrains Mono, bundled), an accent colour, rounder and roomier widgets, and light and
//! dark palettes. Plots ask [`dark`] for their background, grid and text colours.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Theme, ThemePreference, Vec2};
use std::sync::atomic::{AtomicBool, Ordering};

/// The accent: selected tabs, focused fields, sliders, links.
pub const ACCENT: Color32 = Color32::from_rgb(47, 111, 222);
const ACCENT_DARK: Color32 = Color32::from_rgb(110, 160, 245);

static DARK: AtomicBool = AtomicBool::new(false);
static TOUCH: AtomicBool = AtomicBool::new(false);

/// Whether the screen is a touch screen (set by [`touch`]).
pub fn is_touch() -> bool {
    TOUCH.load(Ordering::Relaxed)
}

/// Whether this frame is drawn in the dark palette.
pub fn dark() -> bool {
    DARK.load(Ordering::Relaxed)
}

/// Fonts and styles for both palettes, and the theme from the settings. Call once at start-up.
pub fn install(ctx: &egui::Context) {
    use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
    let font = |name: &str, data: &'static [u8], family: FontFamily| {
        ctx.add_font(FontInsert {
            name: name.into(),
            data: egui::FontData::from_static(data),
            families: vec![InsertFontFamily { family, priority: FontPriority::Highest }],
        });
    };
    font("Inter", include_bytes!("fonts/Inter-Regular.ttf"), FontFamily::Proportional);
    font("JetBrains Mono", include_bytes!("fonts/JetBrainsMono-Regular.ttf"), FontFamily::Monospace);

    ctx.set_visuals_of(Theme::Light, light());
    ctx.set_visuals_of(Theme::Dark, dark_visuals());
    ctx.all_styles_mut(|s| {
        use TextStyle::*;
        s.text_styles = [
            (Small, FontId::proportional(11.0)),
            (Body, FontId::proportional(14.0)),
            (Button, FontId::proportional(14.0)),
            (Heading, FontId::proportional(19.0)),
            (Monospace, FontId::monospace(13.0)),
        ]
        .into();
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
        s.spacing.button_padding = Vec2::new(8.0, 4.0);
        s.spacing.interact_size.y = 22.0;
        s.spacing.menu_margin = egui::Margin::same(8);
        s.spacing.window_margin = egui::Margin::same(12);
    });
    apply_preference(ctx, &crate::settings::get().theme);
}

/// Bigger targets for fingers (Android).
#[allow(dead_code)]
pub fn touch(ctx: &egui::Context) {
    TOUCH.store(true, Ordering::Relaxed);
    ctx.all_styles_mut(|s| {
        s.spacing.interact_size = Vec2::new(44.0, 36.0);
        s.spacing.button_padding = Vec2::new(12.0, 7.0);
        s.spacing.item_spacing = Vec2::new(10.0, 8.0);
        s.spacing.icon_width = 20.0;
        s.spacing.slider_rail_height = 6.0;
        s.interaction.resize_grab_radius_side = 10.0;
    });
}

/// "light", "dark", or anything else to follow the system.
pub fn apply_preference(ctx: &egui::Context, pref: &str) {
    ctx.set_theme(match pref {
        "light" => ThemePreference::Light,
        "dark" => ThemePreference::Dark,
        _ => ThemePreference::System,
    });
}

/// Call at the start of each frame, before anything is painted.
pub fn frame(ctx: &egui::Context) {
    DARK.store(ctx.theme() == Theme::Dark, Ordering::Relaxed);
}

fn rounded(v: &mut egui::Visuals) {
    let r = CornerRadius::same(6);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
    }
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.striped = false;
}

fn light() -> egui::Visuals {
    let mut v = egui::Visuals::light();
    rounded(&mut v);
    let rgb = Color32::from_rgb;
    v.panel_fill = rgb(247, 248, 250);
    v.window_fill = rgb(255, 255, 255);
    v.extreme_bg_color = rgb(255, 255, 255);
    v.faint_bg_color = rgb(240, 243, 247);
    v.code_bg_color = rgb(238, 241, 245);
    v.window_stroke = Stroke::new(1.0, rgb(218, 223, 230));
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, rgb(222, 226, 232));
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, rgb(40, 44, 52));
    v.widgets.inactive.bg_fill = rgb(232, 236, 242);
    v.widgets.inactive.weak_bg_fill = rgb(232, 236, 242);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, rgb(30, 34, 42));
    v.widgets.hovered.bg_fill = rgb(220, 227, 237);
    v.widgets.hovered.weak_bg_fill = rgb(220, 227, 237);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, rgb(170, 190, 225));
    v.widgets.active.bg_fill = rgb(205, 216, 232);
    v.widgets.active.weak_bg_fill = rgb(205, 216, 232);
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(47, 111, 222, 60);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    v
}

fn dark_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    rounded(&mut v);
    let rgb = Color32::from_rgb;
    v.panel_fill = rgb(28, 31, 36);
    v.window_fill = rgb(34, 38, 44);
    v.extreme_bg_color = rgb(20, 22, 26);
    v.faint_bg_color = rgb(40, 45, 52);
    v.code_bg_color = rgb(40, 44, 52);
    v.window_stroke = Stroke::new(1.0, rgb(58, 64, 74));
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, rgb(52, 58, 68));
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, rgb(214, 218, 226));
    v.widgets.inactive.bg_fill = rgb(46, 51, 60);
    v.widgets.inactive.weak_bg_fill = rgb(46, 51, 60);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, rgb(226, 230, 236));
    v.widgets.hovered.bg_fill = rgb(58, 65, 77);
    v.widgets.hovered.weak_bg_fill = rgb(58, 65, 77);
    v.widgets.active.bg_fill = rgb(70, 80, 96);
    v.widgets.active.weak_bg_fill = rgb(70, 80, 96);
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(110, 160, 245, 70);
    v.selection.stroke = Stroke::new(1.0, ACCENT_DARK);
    v.hyperlink_color = ACCENT_DARK;
    v
}

// colours for the plots and 3D views, which paint their own backgrounds

pub fn plot_bg() -> Color32 {
    if dark() { Color32::from_rgb(24, 27, 31) } else { Color32::from_rgb(253, 253, 251) }
}
pub fn view_bg() -> Color32 {
    if dark() { Color32::from_rgb(20, 22, 26) } else { Color32::from_rgb(246, 248, 252) }
}
pub fn grid() -> Color32 {
    if dark() { Color32::from_rgb(66, 72, 82) } else { Color32::from_rgb(205, 208, 214) }
}
pub fn ink() -> Color32 {
    if dark() { Color32::from_rgb(222, 226, 233) } else { Color32::from_rgb(40, 40, 50) }
}
/// Axis numbers and other secondary text.
pub fn ink_soft() -> Color32 {
    if dark() { Color32::from_rgb(150, 156, 168) } else { Color32::from_rgb(96, 98, 106) }
}
/// Behind text drawn over a plot.
pub fn label_bg() -> Color32 {
    if dark() { Color32::from_rgba_unmultiplied(24, 27, 31, 230) } else { Color32::from_rgba_unmultiplied(255, 255, 255, 230) }
}
/// The 3D pattern's mesh lines.
pub fn mesh() -> Color32 {
    if dark() { Color32::from_rgba_unmultiplied(235, 235, 240, 70) } else { Color32::from_rgba_unmultiplied(30, 30, 30, 110) }
}
