//! Android entry point for wireant (proof of concept). The desktop sources are compiled in
//! unchanged from ../src; only start-up differs: no command line, no MCP server, OpenGL ES
//! through glow, and the app's private storage standing in for the home folder.
#![allow(dead_code)]

#[path = "../../src/app.rs"] mod app;
#[path = "../../src/maa.rs"] mod maa;
#[path = "../../src/matching.rs"] mod matching;
#[path = "../../src/mcp.rs"] mod mcp;
#[path = "../../src/model.rs"] mod model;
#[path = "../../src/mom.rs"] mod mom;
#[path = "../../src/nec.rs"] mod nec;
#[path = "../../src/nec_file.rs"] mod nec_file;
#[path = "../../src/nearfield.rs"] mod nearfield;
#[path = "../../src/exposure.rs"] mod exposure;
#[path = "../../src/exposure3d.rs"] mod exposure3d;
#[path = "../../src/stack.rs"] mod stack;
#[path = "../../src/greatcircle.rs"] mod greatcircle;
#[path = "../../src/wm.rs"] mod wm;
#[path = "../../src/ez.rs"] mod ez;
#[path = "../../src/file_name.rs"] mod file_name;
#[path = "../../src/help.rs"] mod help;
#[path = "../../src/optimize.rs"] mod optimize;
#[path = "../../src/plots.rs"] mod plots;
#[path = "../../src/progress.rs"] mod progress;
#[path = "../../src/tools.rs"] mod tools;
#[path = "../../src/pdf.rs"] mod pdf;
#[path = "../../src/report.rs"] mod report;
#[path = "../../src/settings.rs"] mod settings;
#[path = "../../src/theme.rs"] mod theme;
#[path = "../../src/num.rs"] mod num;
mod files;

use eframe::egui;
use winit::platform::android::activity::AndroidApp;

// the desktop main.rs keeps these; nothing else calls them, but keep the names in case
fn log_error(msg: &str) {
    log::error!("{msg}");
}

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("wireant"));
    std::panic::set_hook(Box::new(|info| log::error!("panic: {info}")));
    // settings.txt, the cache and saved files live in the app's private storage
    if let Some(dir) = app.internal_data_path() {
        // SAFETY: still single-threaded
        unsafe {
            std::env::set_var("HOME", &dir);
            std::env::set_var("XDG_CONFIG_HOME", dir.join(".config"));
            std::env::set_var("XDG_CACHE_HOME", dir.join(".cache"));
            std::env::set_var("XDG_DATA_HOME", dir.join(".local/share"));
        }
    }
    let options = eframe::NativeOptions {
        android_app: Some(app.clone()),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    let r = eframe::run_native("wireant", options, Box::new(|cc| {
        cc.egui_ctx.set_fonts(fonts());
        files::set_context(&cc.egui_ctx);
        let inner = app::App::new(cc, None, None, None);
        // after App::new, which sets the shared theme: finger-sized targets
        theme::touch(&cc.egui_ctx);
        Ok(Box::new(Insets { inner, android: app, docs: files::Docs::default() }))
    }));
    if let Err(e) = r {
        log::error!("eframe: {e}");
    }
    // the event loop can't start again in this process, so don't leave it behind
    std::process::exit(0);
}

/// egui's bundled fonts plus fallbacks for the symbols the desktop gets from system fonts
/// (● ↶ → ∥ ⊥ ✘): bundled Hack, then Android's own Noto Sans Symbols.
fn fonts() -> egui::FontDefinitions {
    use egui::FontFamily;
    let mut f = egui::FontDefinitions::default();
    let mut extra = vec!["Hack".to_owned()];
    if let Ok(data) = std::fs::read("/system/fonts/NotoSansSymbols-Regular-Subsetted.ttf") {
        f.font_data.insert("android-symbols".to_owned(), std::sync::Arc::new(egui::FontData::from_owned(data)));
        extra.push("android-symbols".to_owned());
    }
    for fam in [FontFamily::Proportional, FontFamily::Monospace] {
        let list = f.families.entry(fam).or_default();
        for e in &extra {
            if !list.contains(e) {
                list.push(e.clone());
            }
        }
    }
    f
}

/// The desktop App, kept clear of Android's status and navigation bars: the activity draws
/// edge to edge, and egui leaves the safe-area insets free when it is told them.
struct Insets {
    inner: app::App,
    android: AndroidApp,
    docs: files::Docs,
}

impl eframe::App for Insets {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // empty panels the height of the system bars, before the app's own panels
        let m = ui.ctx().input(|i| i.safe_area_insets()).0;
        egui::Panel::top("android_status_bar").exact_size(m.top).frame(egui::Frame::NONE).show(ui, |_| {});
        egui::Panel::bottom("android_nav_bar").exact_size(m.bottom).frame(egui::Frame::NONE).show(ui, |_| {});
        self.inner.ui(ui, frame);
        self.docs.frame(&self.android, &mut self.inner);
        // File ▸ Quit: finish the activity rather than leave it running with no event loop
        // and Android's Back gesture, which the native activity otherwise swallows
        let back = ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::BrowserBack));
        if back || ui.ctx().input(|i| i.viewport().close_requested()) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
            files::quit(&self.android);
        }
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let Some(w) = self.android.native_window() else { return };
        let (ww, wh) = (w.width() as f32, w.height() as f32);
        let r = self.android.content_rect();
        let ppp = ctx.pixels_per_point();
        let top = r.top.max(0) as f32;
        let mut bottom = (wh - r.bottom as f32).max(0.0);
        // content_rect can come back as the whole window on Android 15+ (edge to edge); keep
        // at least the usual bar heights there. Older versions report the bars correctly.
        let edge_to_edge = self.android.config().sdk_version() >= 35;
        let top = if top < 1.0 && edge_to_edge { 48.0 * ppp } else { top };
        if bottom < 1.0 && edge_to_edge { bottom = 48.0 * ppp; }
        raw.safe_area_insets = Some(egui::SafeAreaInsets(egui::epaint::MarginF32 {
            top: top / ppp,
            bottom: bottom / ppp,
            left: (r.left.max(0) as f32) / ppp,
            right: ((ww - r.right as f32).max(0.0)) / ppp,
        }));
    }
}
