//! Android's file picker (Storage Access Framework) for File → Open, Save as and the
//! reports, through the Java side in ../java/radio/wireant/MainActivity.java.
//!
//! The program itself only knows paths. A picked document is copied into the app's
//! storage and opened from there; a saved file is written to the app's `exports` folder
//! (see rfd-stub) and copied to the document the user picks. Each local copy stays linked
//! to its document, so when the program saves it again the change is copied back.
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use eframe::egui;
use jni::objects::{JClass, JObject, JString, JValue};
use jni::sys::jint;
use jni::{JNIEnv, JavaVM};
use winit::platform::android::activity::AndroidApp;

const PICK: i32 = 1;

struct DocResult {
    kind: i32,
    uri: Option<String>,
    path: Option<String>,
    error: Option<String>,
}

static RESULTS: Mutex<Vec<DocResult>> = Mutex::new(Vec::new());
static CTX: OnceLock<egui::Context> = OnceLock::new();

/// Lets a result arriving from Java wake the UI, which may be idle.
pub fn set_context(ctx: &egui::Context) {
    let _ = CTX.set(ctx.clone());
}

fn opt_string(env: &mut JNIEnv, s: &JString) -> Option<String> {
    if s.is_null() {
        None
    } else {
        env.get_string(s).ok().map(Into::into)
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_radio_wireant_MainActivity_nativeResult<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    kind: jint,
    uri: JString<'l>,
    path: JString<'l>,
    error: JString<'l>,
) {
    let r = DocResult { kind, uri: opt_string(&mut env, &uri), path: opt_string(&mut env, &path), error: opt_string(&mut env, &error) };
    RESULTS.lock().unwrap().push(r);
    if let Some(ctx) = CTX.get() {
        ctx.request_repaint();
    }
}

struct Link {
    path: PathBuf,
    uri: String,
    mtime: Option<SystemTime>,
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// The file the program just wrote for a save request: `path`, or, when it added an
/// extension or changed the name, the newest file in the same folder.
fn written(path: &Path) -> PathBuf {
    if path.exists() {
        return path.to_path_buf();
    }
    let newest = path.parent().and_then(|d| std::fs::read_dir(d).ok()).and_then(|rd| {
        rd.flatten().filter_map(|e| Some((mtime(&e.path())?, e.path()))).max_by_key(|(t, _)| *t).map(|(_, p)| p)
    });
    newest.unwrap_or_else(|| path.to_path_buf())
}

#[derive(Default)]
pub struct Docs {
    links: Vec<Link>,
}

impl Docs {
    /// Hands queued requests to the picker, acts on its answers and copies saved files back
    /// to their documents. Call once a frame, after the program's own ui.
    pub fn frame(&mut self, android: &AndroidApp, app: &mut crate::app::App) {
        for r in rfd::take_requests() {
            let res = match r {
                rfd::Request::Pick { title: None } => call(android, |env, act| env.call_method(act, "pickFile", "()V", &[]).map(|_| ())),
                rfd::Request::Pick { title: Some(t) } => {
                    app.show_problem(format!("{t}: not available on Android."));
                    Ok(())
                }
                rfd::Request::Export { path, .. } => {
                    let p = written(&path);
                    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    call(android, |env, act| {
                        let n = env.new_string(&name)?;
                        let s = env.new_string(p.to_string_lossy())?;
                        env.call_method(act, "createFile", "(Ljava/lang/String;Ljava/lang/String;)V", &[JValue::from(&n), JValue::from(&s)]).map(|_| ())
                    })
                }
            };
            if let Err(e) = res {
                app.show_problem(format!("The file picker didn't start: {e}"));
            }
        }
        let results = std::mem::take(&mut *RESULTS.lock().unwrap());
        for r in results {
            let verb = if r.kind == PICK { "open" } else { "save" };
            match (r.error, r.uri, r.path) {
                (Some(e), _, _) => app.show_problem(format!("Couldn't {verb} the file: {e}")),
                (None, Some(uri), Some(path)) => {
                    let path = PathBuf::from(path);
                    self.link(&path, uri);
                    if r.kind == PICK {
                        app.open_path(path);
                    }
                }
                _ => {} // cancelled
            }
        }
        // the program saved a linked file again (File → Save): copy it to its document
        for i in 0..self.links.len() {
            let t = mtime(&self.links[i].path);
            if t.is_some() && t != self.links[i].mtime {
                self.links[i].mtime = t;
                let (uri, path) = (self.links[i].uri.clone(), self.links[i].path.to_string_lossy().into_owned());
                let res = call(android, |env, act| {
                    let u = env.new_string(&uri)?;
                    let p = env.new_string(&path)?;
                    let v = env.call_method(act, "writeUri", "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;", &[JValue::from(&u), JValue::from(&p)])?.l()?;
                    Ok(if v.is_null() { None } else { Some(String::from(env.get_string(&JString::from(v))?)) })
                });
                match res {
                    Ok(None) => {}
                    Ok(Some(e)) => app.show_problem(format!("Saved here, but couldn't copy it to the original file: {e}")),
                    Err(e) => app.show_problem(format!("Saved here, but couldn't copy it to the original file: {e}")),
                }
            }
        }
    }

    fn link(&mut self, path: &Path, uri: String) {
        self.links.retain(|l| l.path != path);
        self.links.push(Link { path: path.to_path_buf(), uri, mtime: mtime(path) });
    }
}

/// Closes the activity; MainActivity.onDestroy then ends the process.
pub fn quit(android: &AndroidApp) {
    if let Err(e) = call(android, |env, act| env.call_method(act, "quit", "()V", &[]).map(|_| ())) {
        log::error!("quit: {e}");
        std::process::exit(0);
    }
}

/// Runs f with a JNI env on this thread and the activity object; clears any Java exception.
fn call<T>(android: &AndroidApp, f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<T>) -> jni::errors::Result<T> {
    // SAFETY: both pointers come from android-activity and live as long as the app
    let vm = unsafe { JavaVM::from_raw(android.vm_as_ptr() as *mut jni::sys::JavaVM)? };
    let mut env = vm.attach_current_thread_permanently()?;
    let act = unsafe { JObject::from_raw(android.activity_as_ptr() as jni::sys::jobject) };
    let r = f(&mut env, &act);
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    r
}
