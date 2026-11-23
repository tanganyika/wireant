//! Stand-in for the `rfd` crate on Android, which has no native dialogs in rfd.
//!
//! Android's file picker answers later, in another activity, so these calls queue a
//! [`Request`] that the app's Android wrapper hands to the picker on the next frame:
//! - `pick_file` returns `None` now; the wrapper opens the chosen file when it arrives.
//! - `save_file` returns a path in the app's own `exports` folder at once, so the program
//!   writes the file as usual; the wrapper then asks where to put it and copies it there.
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub enum Request {
    /// Open a file. `title` is the dialog title, if the program set one (only untitled
    /// picks are antenna files; the NEC program picker has no meaning on Android).
    Pick { title: Option<String> },
    /// Copy the file just written at `path` to a place the user picks, named `name`.
    Export { path: PathBuf, name: String },
}

static REQUESTS: Mutex<Vec<Request>> = Mutex::new(Vec::new());

/// The requests made since the last call.
pub fn take_requests() -> Vec<Request> {
    std::mem::take(&mut *REQUESTS.lock().unwrap())
}

fn push(r: Request) {
    REQUESTS.lock().unwrap().push(r);
}

fn exports() -> Option<PathBuf> {
    let d = PathBuf::from(std::env::var_os("HOME")?).join("exports");
    std::fs::create_dir_all(&d).ok()?;
    Some(d)
}

#[derive(Default, Clone)]
pub struct FileDialog {
    name: Option<String>,
    title: Option<String>,
}

impl FileDialog {
    pub fn new() -> Self { Self::default() }
    pub fn set_title(mut self, t: impl Into<String>) -> Self { self.title = Some(t.into()); self }
    pub fn add_filter(self, _n: impl Into<String>, _e: &[impl ToString]) -> Self { self }
    pub fn set_directory<P: AsRef<Path>>(self, _p: P) -> Self { self }
    pub fn set_file_name(mut self, n: impl Into<String>) -> Self { self.name = Some(n.into()); self }
    pub fn pick_file(self) -> Option<PathBuf> {
        push(Request::Pick { title: self.title });
        None
    }
    pub fn save_file(self) -> Option<PathBuf> {
        let name = self.name.unwrap_or_else(|| "antenna.maa".into());
        let path = exports()?.join(&name);
        push(Request::Export { path: path.clone(), name });
        Some(path)
    }
}

pub enum MessageLevel { Info, Warning, Error }
pub enum MessageButtons { Ok, OkCancel, YesNo }
#[derive(PartialEq, Eq)]
pub enum MessageDialogResult { Ok, Cancel, Yes, No }

pub struct MessageDialog;
impl MessageDialog {
    pub fn new() -> Self { MessageDialog }
    pub fn set_level(self, _l: MessageLevel) -> Self { self }
    pub fn set_title(self, _t: impl Into<String>) -> Self { self }
    pub fn set_description(self, _d: impl Into<String>) -> Self { self }
    pub fn set_buttons(self, _b: MessageButtons) -> Self { self }
    pub fn show(self) -> MessageDialogResult { MessageDialogResult::Ok }
}
