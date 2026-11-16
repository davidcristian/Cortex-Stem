//! Opening `dock.css` with the default app for `.css`, or with Notepad.

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};

use stem_core::LaunchTarget;

/// Notepad: what [`open_file`] opens `file` with when the default app for
/// `.css` does not. Windows installs it by default but lets it be removed,
/// so [`open_file`] still fails when Notepad does not start either. Its
/// full path under `system_root` (`%SystemRoot%`) when that is known, so
/// the shell does not look for a `notepad.exe` anywhere else first; the
/// bare name otherwise. Built as a string with `\`, so it reads the same
/// on any host.
pub fn notepad_for(file: &Path, system_root: Option<&OsStr>) -> LaunchTarget {
    let root = system_root.map(OsStr::to_string_lossy);
    let path = match root
        .as_deref()
        .map(|r| r.trim().trim_end_matches(['\\', '/']))
    {
        Some(root) if !root.is_empty() => format!("{root}\\System32\\notepad.exe"),
        _ => "notepad.exe".to_owned(),
    };
    LaunchTarget::Exe {
        path: PathBuf::from(path),
        args: vec![file.to_string_lossy().into_owned()],
        working_dir: None,
    }
}

/// What [`open_file`] opened `dock.css` with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opener {
    /// The app Windows opens a `.css` file with (a double click's).
    DefaultApp,
    /// Notepad, because the default app did not open it.
    Notepad,
}

impl fmt::Display for Opener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::DefaultApp => "with its default app",
            Self::Notepad => "in Notepad",
        })
    }
}

/// Open `file` through `launch` (`Platform::launch`): first as a `Uri`
/// target, which the shell opens with the default app for `.css` as a
/// double click would; when that fails (on a stock Windows there may be no
/// such app, and the shell, asked without UI, refuses instead of offering
/// its "How do you want to open this file?" picker), with `notepad`
/// ([`notepad_for`]), logged at `info`. `Err` is the command's error
/// string, naming both failures. Blocking (the shell).
pub fn open_file(
    file: &Path,
    notepad: &LaunchTarget,
    mut launch: impl FnMut(&LaunchTarget) -> Result<(), String>,
) -> Result<Opener, String> {
    let default_app = LaunchTarget::Uri {
        uri: file.to_string_lossy().into_owned(),
    };
    let Err(first) = launch(&default_app) else {
        return Ok(Opener::DefaultApp);
    };
    log::info!(
        "dock.css: the default app for .css did not open {} ({first}); opening it in Notepad",
        file.display()
    );
    match launch(notepad) {
        Ok(()) => Ok(Opener::Notepad),
        Err(second) => Err(format!(
            "could not open {} with its default app ({first}) or with Notepad ({second})",
            file.display()
        )),
    }
}
