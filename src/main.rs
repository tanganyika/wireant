// no console window behind the program on Windows
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod maa;
mod matching;
mod mcp;
mod model;
mod mom;
mod nec;
mod nec_file;
mod nearfield;
mod exposure;
mod exposure3d;
mod stack;
mod greatcircle;
mod wm;
mod ez;
mod file_name;
mod help;
mod optimize;
mod plots;
mod progress;
mod tools;
mod pdf;
mod report;
mod settings;
mod theme;
mod num;
mod dem;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut shot = None;
    let mut tab = None;
    let mut open = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => shot = args.next().map(PathBuf::from),
            "--tab" => tab = args.next().and_then(|t| app::Tab::from_name(&t)),
            "--mcp" => {
                // MCP server on stdin/stdout, no window
                if let Err(e) = mcp::run() {
                    eprintln!("wireant --mcp: {e}");
                    std::process::exit(1);
                }
                return Ok(());
            }
            "--help" | "-h" => {
                println!("wireant [antenna.maa] [--tab geometry|view|calculate|far|sweep|optimizer] [--screenshot out.png]");
                println!("wireant --mcp    run as an MCP server on stdin/stdout (no window)");
                return Ok(());
            }
            _ => open = Some(PathBuf::from(a)),
        }
    }
    install_panic_hook();
    // wgpu (DirectX 12 / Vulkan / Metal / OpenGL) by default; plain OpenGL (glow) when wgpu
    // can't start, e.g. old graphics drivers, virtual machines, remote desktop.
    let forced = std::env::var("WIREANT_RENDERER").ok().map(|r| r.to_ascii_lowercase());
    // A graphics driver can crash the whole process while wgpu starts (seen: Intel's Vulkan
    // driver igvk64.dll, 0xC0000005), which no error return catches. A marker file is written
    // before wgpu starts and removed once the window is up; if it is still there, the last
    // wgpu start crashed, so use OpenGL. WIREANT_RENDERER=wgpu tries wgpu again.
    let marker_path = log_path().map(|p| p.with_file_name("wgpu-starting"));
    let crashed_before = forced.is_none() && marker_path.as_ref().is_some_and(|m| m.exists());
    if crashed_before {
        log_error("the last start with wgpu crashed; starting with OpenGL (set WIREANT_RENDERER=wgpu to try wgpu again)");
    }
    let glow = forced.as_deref() == Some("glow") || crashed_before;
    // On Windows use DirectX 12 only: it works on every Windows 10/11 GPU, while old Vulkan
    // drivers (Intel especially) crash. WGPU_BACKEND still overrides this.
    #[cfg(windows)]
    if std::env::var_os("WGPU_BACKEND").is_none() {
        // SAFETY: still single-threaded, nothing else reads the environment yet
        unsafe { std::env::set_var("WGPU_BACKEND", "dx12") };
    }
    // only a wgpu start owns the marker; an OpenGL start must leave it in place
    let marker = if glow { None } else { marker_path };
    if let Some(m) = &marker {
        if let Some(dir) = m.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(m, "wgpu start in progress; if this file stays, wgpu crashed\n");
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([800.0, 560.0])
            .with_title("wireant")
            .with_app_id("wireant"),
        renderer: if glow { eframe::Renderer::Glow } else { eframe::Renderer::Wgpu },
        ..Default::default()
    };
    let result = eframe::run_native(
        "wireant",
        options,
        Box::new({
            let marker = marker.clone();
            move |cc| {
                // the renderer is up (adapter, device and surface made), so wgpu didn't crash
                if let Some(m) = &marker {
                    let _ = std::fs::remove_file(m);
                }
                Ok(Box::new(app::App::new(cc, open, tab, shot)))
            }
        }),
    );
    // a clean error return isn't a crash: the OpenGL retry below handles it
    if let Some(m) = &marker {
        let _ = std::fs::remove_file(m);
    }
    let Err(e) = result else { return Ok(()) };
    if !glow {
        log_error(&format!("wgpu renderer failed, retrying with OpenGL: {e}"));
        // winit allows one event loop per process, so try OpenGL in a fresh one
        if let Ok(exe) = std::env::current_exe() {
            if let Ok(status) = std::process::Command::new(exe).args(std::env::args_os().skip(1)).env("WIREANT_RENDERER", "glow").status() {
                // 0 = closed normally, 1 = start_failed already said why, 101 = the panic hook did;
                // anything else is a crash (e.g. 0xC0000005 in a graphics driver) nobody reported
                match status.code() {
                    Some(0 | 1 | 101) => {}
                    code => {
                        let code = code.map_or("a signal".to_string(), |c| format!("code {c} (0x{:08X})", c as u32));
                        let msg = format!("wireant crashed while starting with OpenGL: it exited with {code}.");
                        log_error(&msg);
                        error_box(&format!("{msg}\n\nDetails are in {}", log_path_text()));
                    }
                }
                std::process::exit(status.code().unwrap_or(1));
            }
        }
    }
    start_failed(&e);
    Err(e)
}

/// The program couldn't open its window: say why, as on Windows there is no console to see it.
fn start_failed(e: &eframe::Error) {
    let msg = format!(
        "wireant couldn't open its window:\n\n{e}\n\nThis usually means the graphics driver doesn't support \
         OpenGL 2 or newer (or DirectX 12 / Vulkan). Updating the graphics driver normally fixes it; \
         in a virtual machine, turn on 3D acceleration."
    );
    log_error(&msg);
    error_box(&msg);
}

/// Where start-up failures and panics are written: %LOCALAPPDATA%\wireant\startup-error.txt,
/// ~/.cache/wireant/startup-error.txt, ~/Library/Caches/wireant/startup-error.txt.
fn log_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("wireant").join("startup-error.txt"))
}

fn log_path_text() -> String {
    log_path().map_or("(no cache folder)".to_string(), |p| p.display().to_string())
}

/// Print `msg` and append it to the log with the version, time, renderer and OS, so a user
/// can send the file. The log is cut back when it passes 256 KB.
fn log_error(msg: &str) {
    eprintln!("{msg}");
    let Some(path) = log_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&path).map(|m| m.len() > 256 * 1024).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let renderer = std::env::var("WIREANT_RENDERER").unwrap_or_else(|_| "wgpu".into());
    let entry = format!(
        "=== wireant {} | unix time {secs} | renderer {renderer} | {} {} ===\n{msg}\n\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(entry.as_bytes());
    }
}

fn error_box(msg: &str) {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("wireant")
        .set_description(msg)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

/// A panic would otherwise end the program without a word on Windows (no console), so log it
/// with a backtrace, and on the main thread say so in a message box. Panics on worker threads
/// (sweeps, optimizer) are only logged: the program carries on and reports the failed job.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed").to_string();
        let what = info.payload().downcast_ref::<&str>().map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".into());
        let place = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
        let backtrace = std::backtrace::Backtrace::force_capture();
        log_error(&format!("panic on thread '{name}'{place}: {what}\n\nbacktrace:\n{backtrace}"));
        if name == "main" && std::env::var_os("WIREANT_NO_ERROR_BOX").is_none() {
            error_box(&format!(
                "wireant stopped because of an internal error:\n\n{what}{place}\n\n\
                 Details were saved to {}. Please send that file with your report.",
                log_path_text()
            ));
        }
    }));
}
