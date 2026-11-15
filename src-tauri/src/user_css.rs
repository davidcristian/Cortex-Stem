//! The user's stylesheet (`docs/ipc/themes.md`, `docs/THEMING.md`):
//! `dock.css` in the settings file's folder
//! (`%APPDATA%\dev.davidcristian.stem\dock.css` for an installed Stem).
//!
//! Read in `setup`, before the dock windows exist, and again on
//! `reload_user_css`; never on its own (no file watching). The text the
//! page gets ([`UserCss::css`]) is the file's, a leading byte order mark
//! dropped, or `None` when the file does not exist (an `info` line: the
//! normal case), cannot be read, is not UTF-8 or is larger than
//! [`MAX_BYTES`] (a `warn` line each, saying which). The read is bounded:
//! the file's size is checked before and while reading, so a huge or
//! growing file costs at most [`MAX_BYTES`] + 1 bytes.
//!
//! `open_user_css` writes [`TEMPLATE`] when there is no file yet (never over
//! an existing one) and hands the path to `Platform::launch`, which opens it
//! with the default app for `.css`, or with Notepad when that fails
//! ([`open_file`]): a stock Windows may have no app for `.css`, and the
//! shell, asked without UI, then refuses instead of offering its picker.
//!
//! Reloads are serialised ([`UserCssState::reload_and`]): the read, the
//! stored result and what the docks are sent always belong to the same
//! read, and the last Reload pressed is the one that stays.
//!
//! A run without a settings file (the mock platform, a profile that could
//! not be used) has no `dock.css` either: the path is a note in parentheses,
//! nothing is read and `open_user_css` rejects.

use std::ffi::OsStr;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read as _, Write as _};
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use stem_core::{LaunchTarget, UserCss};
use tauri::{AppHandle, Emitter, EventTarget};

/// The stylesheet's file name, next to `settings.json`.
pub const FILE_NAME: &str = "dock.css";

/// The largest file Stem reads (64 KiB); anything larger is ignored whole.
pub const MAX_BYTES: u64 = 64 * 1024;

/// `UserCss::path` when nothing is saved this run.
const NO_FILE_DEMO: &str = "(demo: no dock.css)";
const NO_FILE: &str = "(no dock.css this session)";

/// What `open_user_css` writes when there is no `dock.css` yet: every line
/// commented out, so creating it changes nothing until the user edits it.
/// Lists the stable variables (`docs/THEMING.md`, with the defaults of
/// every preset); the values shown are the `standard` preset's in dark
/// mode.
pub const TEMPLATE: &str = r#"/* dock.css: your own styles for Stem's dock.
 *
 * Stem reads this file when it starts and when you press Reload in
 * Settings > Appearance; it does not watch it. Save it as UTF-8 and keep it
 * under 64 KiB, or Stem ignores it (its log says why). The dock applies it
 * after its own styles, as CSS only: nothing in it runs as a script, and
 * url() loads nothing from your disk or the web (data: URLs work).
 *
 * The variables below are the stable way in: they keep working across
 * versions. Anything else you style here (class names, layout) may change
 * with any update. Set them on .dock-stage for every theme preset, or on
 * .dock-stage[data-theme="standard"], [data-theme="solid"] or
 * [data-theme="minimal"] for one of them; put html[data-theme="light"] or
 * html[data-theme="dark"] in front to narrow a rule to light or dark mode.
 * docs/THEMING.md in Stem's repository lists every default.
 *
 * Remove the comment marks around a line to use it.
 */

.dock-stage {
  /* The panel's background: a colour or a gradient. */
  /* --stem-panel-fill: rgb(28 28 30 / 0.72); */

  /* What the panel does to the desktop behind it (a backdrop-filter, or none). */
  /* --stem-panel-blur: blur(24px) saturate(1.4); */

  /* The 1 px line around the panel (a colour). */
  /* --stem-panel-hairline: rgb(255 255 255 / 0.12); */

  /* The panel's shadow (a box-shadow, or none). */
  /* --stem-panel-shadow: 0 12px 32px rgb(0 0 0 / 0.45); */

  /* The panel's corner radius (a length; Settings > Corner radius by default). */
  /* --stem-panel-radius: 22px; */

  /* The lines between the groups of items (a colour). */
  /* --stem-separator: rgb(255 255 255 / 0.22); */

  /* The running-app dots (a colour; the Windows accent colour by default). */
  /* --stem-dot: #0078d4; */

  /* The app-name pills: background and text (colours). */
  /* --stem-pill-fill: rgb(40 40 42 / 0.94); */
  /* --stem-pill-text: #ffffff; */
}

/* For example, a white panel for the Solid theme in light mode only:

html[data-theme="light"] .dock-stage[data-theme="solid"] {
  --stem-panel-fill: #ffffff;
}
*/
"#;

/// Why a file's text is not used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Larger than [`MAX_BYTES`]: the size seen (at least `MAX_BYTES + 1`).
    TooLarge(u64),
    NotUtf8,
    /// Opening or reading failed, or the path is not a file.
    Unreadable(String),
}

/// What reading `dock.css` gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    Missing,
    Css(String),
    Refused(Refusal),
}

/// Read the stylesheet at `path`, bounded by [`MAX_BYTES`]. Blocking (disk).
pub fn read_file(path: &Path) -> ReadOutcome {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::NotFound => return ReadOutcome::Missing,
        Err(e) => return ReadOutcome::Refused(Refusal::Unreadable(e.to_string())),
    };
    match file.metadata() {
        Ok(meta) if !meta.is_file() => {
            return ReadOutcome::Refused(Refusal::Unreadable("not a file".to_owned()));
        }
        Ok(meta) if meta.len() > MAX_BYTES => {
            return ReadOutcome::Refused(Refusal::TooLarge(meta.len()));
        }
        Ok(_) => {}
        Err(e) => return ReadOutcome::Refused(Refusal::Unreadable(e.to_string())),
    }
    // The file may have grown since the size check: read one byte past the
    // limit at most, which is enough to tell.
    let mut bytes = Vec::new();
    if let Err(e) = file.take(MAX_BYTES + 1).read_to_end(&mut bytes) {
        return ReadOutcome::Refused(Refusal::Unreadable(e.to_string()));
    }
    match decode(bytes) {
        Ok(css) => ReadOutcome::Css(css),
        Err(refusal) => ReadOutcome::Refused(refusal),
    }
}

/// The file's bytes as the page's CSS: at most [`MAX_BYTES`], UTF-8, with a
/// leading byte order mark (which Notepad may write) dropped, since inside
/// a `<style>` element it would become part of the first selector.
pub fn decode(bytes: Vec<u8>) -> Result<String, Refusal> {
    let len = bytes.len() as u64;
    if len > MAX_BYTES {
        return Err(Refusal::TooLarge(len));
    }
    let text = String::from_utf8(bytes).map_err(|_| Refusal::NotUtf8)?;
    Ok(match text.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_owned(),
        None => text,
    })
}

/// The log line for `outcome` (`warn` for a refusal, `info` otherwise) and
/// the CSS the page gets.
fn report(path: &Path, outcome: ReadOutcome) -> Option<String> {
    match outcome {
        ReadOutcome::Missing => {
            log::info!(
                "dock.css: none at {}; the dock uses its own styles only",
                path.display()
            );
            None
        }
        ReadOutcome::Css(css) => {
            log::info!("dock.css: {} bytes from {}", css.len(), path.display());
            Some(css)
        }
        ReadOutcome::Refused(Refusal::TooLarge(size)) => {
            log::warn!(
                "dock.css at {} is ignored: it is {size} bytes, more than the {MAX_BYTES} (64 KiB) Stem reads",
                path.display()
            );
            None
        }
        ReadOutcome::Refused(Refusal::NotUtf8) => {
            log::warn!(
                "dock.css at {} is ignored: it is not UTF-8 text (save it as UTF-8)",
                path.display()
            );
            None
        }
        ReadOutcome::Refused(Refusal::Unreadable(e)) => {
            log::warn!(
                "dock.css at {} is ignored: it cannot be read ({e})",
                path.display()
            );
            None
        }
    }
}

/// `dock.css` next to the settings file, or `None` when there is no
/// settings file this run or it has no folder.
pub fn path_for(settings_file: Option<&Path>) -> Option<PathBuf> {
    settings_file
        .and_then(Path::parent)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(FILE_NAME))
}

/// Write [`TEMPLATE`] to `path` unless a file is already there (it is never
/// overwritten). Returns whether it wrote one. Blocking (disk).
pub fn write_template_if_missing(path: &Path) -> std::io::Result<bool> {
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::AlreadyExists => return Ok(false),
        Err(e) => return Err(e),
    };
    file.write_all(TEMPLATE.as_bytes())?;
    file.sync_all()?;
    Ok(true)
}

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

/// The app's view of `dock.css`, in Tauri's managed state: where it is and
/// what the last read gave (`get_user_css`).
#[derive(Debug)]
pub struct UserCssState {
    /// The file; `None` when nothing is saved this run.
    file: Option<PathBuf>,
    current: Mutex<UserCss>,
    /// Held for a whole reload (read, store, publish), so two at once
    /// cannot finish in the wrong order. Separate from `current`, so
    /// `get_user_css` never waits for the disk.
    reloading: Mutex<()>,
}

impl UserCssState {
    /// The state for a run whose settings file is `settings_file` (`None`
    /// for the in-memory store; `demo` says whether that is the mock
    /// platform). Reads nothing yet: [`UserCssState::reload`] does.
    pub fn new(settings_file: Option<&Path>, demo: bool) -> Self {
        let file = path_for(settings_file);
        let path = match &file {
            Some(file) => file.display().to_string(),
            None if demo => NO_FILE_DEMO.to_owned(),
            None => NO_FILE.to_owned(),
        };
        Self {
            file,
            current: Mutex::new(UserCss { path, css: None }),
            reloading: Mutex::new(()),
        }
    }

    /// What the last read gave (none yet: no CSS).
    pub fn current(&self) -> UserCss {
        self.current.lock().clone()
    }

    /// Read the file again, log what came of it and keep the result.
    /// Blocking (disk): `setup` calls it once, before any dock window
    /// exists, so there is no one to tell.
    pub fn reload(&self) -> UserCss {
        self.reload_and(|_| {})
    }

    /// [`UserCssState::reload`], then `publish` the result (the command
    /// sends it to the docks), all under one lock: a second reload waits
    /// until the first has published, so the stored result and the last
    /// thing the docks were sent are always the same, latest read. Without
    /// it, two Reloads around a save could store and send the new text and
    /// then the old. Blocking (disk): the command runs it on the blocking
    /// pool.
    pub fn reload_and(&self, publish: impl FnOnce(&UserCss)) -> UserCss {
        let _reloading = self.reloading.lock();
        let css = self
            .file
            .as_deref()
            .and_then(|file| report(file, read_file(file)));
        let read = {
            let mut current = self.current.lock();
            current.css = css;
            current.clone()
        };
        publish(&read);
        read
    }

    /// The file `open_user_css` opens, written from [`TEMPLATE`] first if
    /// it does not exist yet. `Err` is the command's error string. Blocking
    /// (disk).
    pub fn file_to_open(&self) -> Result<PathBuf, String> {
        let file = self
            .file
            .as_deref()
            .ok_or_else(|| "settings are not saved to a file in this run".to_owned())?;
        match write_template_if_missing(file) {
            Ok(true) => log::info!("dock.css: wrote the template to {}", file.display()),
            Ok(false) => {}
            Err(e) => {
                return Err(format!(
                    "could not create {} from the template: {e}",
                    file.display()
                ));
            }
        }
        Ok(file.to_path_buf())
    }
}

/// Whether `target` names a dock window (`dock`, `dock-<monitor>`).
fn is_dock_target(target: &EventTarget) -> bool {
    match target {
        EventTarget::Window { label }
        | EventTarget::Webview { label }
        | EventTarget::WebviewWindow { label }
        | EventTarget::AnyLabel { label } => label.starts_with(crate::windows::DOCK_LABEL),
        _ => false,
    }
}

/// Send `css` to every dock window as `dock://user-css`. Listeners a page
/// registers without a target (`listen`, the frontend's way) receive every
/// event whatever the filter, so the settings page would get it too if it
/// listened; it does not, and takes `reload_user_css`'s answer instead.
pub fn emit_to_docks(app: &AppHandle, css: &UserCss) {
    if let Err(e) = app.emit_filter(crate::events::USER_CSS, css, is_dock_target) {
        log::warn!("failed to emit {}: {e}", crate::events::USER_CSS);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("stem-user-css-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Every `--stem-<name>` variable in `text`.
    fn variables(text: &str) -> BTreeSet<String> {
        const PREFIX: &str = "--stem-";
        let mut names = BTreeSet::new();
        let mut rest = text;
        while let Some(at) = rest.find(PREFIX) {
            let tail = &rest[at + PREFIX.len()..];
            let end = tail
                .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
                .unwrap_or(tail.len());
            let name = tail[..end].trim_end_matches('-');
            if !name.is_empty() {
                names.insert(format!("{PREFIX}{name}"));
            }
            rest = &tail[end..];
        }
        names
    }

    #[test]
    fn a_missing_file_is_no_css_and_a_utf8_one_is_its_text_without_a_bom() {
        let dir = scratch_dir("read");
        let file = dir.join(FILE_NAME);
        assert_eq!(read_file(&file), ReadOutcome::Missing);

        std::fs::write(&file, ".dock-stage { --stem-dot: red; }").unwrap();
        assert_eq!(
            read_file(&file),
            ReadOutcome::Css(".dock-stage { --stem-dot: red; }".to_owned())
        );

        std::fs::write(&file, "\u{feff}.dock-stage { --stem-dot: red; }").unwrap();
        assert_eq!(
            read_file(&file),
            ReadOutcome::Css(".dock-stage { --stem-dot: red; }".to_owned())
        );

        std::fs::write(&file, "").unwrap();
        assert_eq!(read_file(&file), ReadOutcome::Css(String::new()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_over_64_kib_or_not_utf8_or_not_a_file_is_refused() {
        let dir = scratch_dir("refuse");
        let file = dir.join(FILE_NAME);

        let limit = usize::try_from(MAX_BYTES).unwrap();
        std::fs::write(&file, "a".repeat(limit)).unwrap();
        assert!(matches!(read_file(&file), ReadOutcome::Css(css) if css.len() == limit));
        std::fs::write(&file, "a".repeat(limit + 1)).unwrap();
        assert_eq!(
            read_file(&file),
            ReadOutcome::Refused(Refusal::TooLarge(MAX_BYTES + 1))
        );

        // UTF-16 (Notepad's "Unicode"), with its byte order mark.
        std::fs::write(&file, [0xff, 0xfe, b'a', 0, b'{', 0, b'}', 0]).unwrap();
        assert_eq!(read_file(&file), ReadOutcome::Refused(Refusal::NotUtf8));

        let folder = dir.join("folder.css");
        std::fs::create_dir_all(&folder).unwrap();
        assert!(matches!(
            read_file(&folder),
            ReadOutcome::Refused(Refusal::Unreadable(_))
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn decode_counts_the_bom_against_the_limit_and_drops_only_a_leading_one() {
        let limit = usize::try_from(MAX_BYTES).unwrap();
        let mut bytes = "\u{feff}".as_bytes().to_vec();
        bytes.extend(std::iter::repeat_n(b'a', limit - 3));
        assert_eq!(decode(bytes.clone()).map(|css| css.len()), Ok(limit - 3));
        bytes.push(b'a');
        assert_eq!(decode(bytes), Err(Refusal::TooLarge(MAX_BYTES + 1)));
        assert_eq!(
            decode("a\u{feff}".as_bytes().to_vec()),
            Ok("a\u{feff}".to_owned())
        );
        assert_eq!(decode(vec![0xc3, 0x28]), Err(Refusal::NotUtf8));
    }

    #[test]
    fn the_state_reads_only_when_asked_and_keeps_the_last_read() {
        let dir = scratch_dir("state");
        let state = UserCssState::new(Some(&dir.join("settings.json")), false);
        let path = dir.join(FILE_NAME).display().to_string();
        let none = UserCss {
            path: path.clone(),
            css: None,
        };
        assert_eq!(state.current(), none);

        std::fs::write(dir.join(FILE_NAME), "a {}").unwrap();
        assert_eq!(state.current(), none, "no read before reload");
        let read = state.reload();
        assert_eq!(read.css.as_deref(), Some("a {}"));
        assert_eq!(state.current(), read);

        // An edit shows only at the next reload; a refused file clears it.
        std::fs::write(dir.join(FILE_NAME), [0xff, 0xfe]).unwrap();
        assert_eq!(state.current(), read);
        assert_eq!(state.reload(), none);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_a_settings_file_there_is_no_dock_css() {
        for (demo, note) in [(true, NO_FILE_DEMO), (false, NO_FILE)] {
            let state = UserCssState::new(None, demo);
            let expected = UserCss {
                path: note.to_owned(),
                css: None,
            };
            assert_eq!(state.current(), expected);
            assert_eq!(state.reload(), expected);
            assert_eq!(
                state.file_to_open(),
                Err("settings are not saved to a file in this run".to_owned())
            );
        }
        assert_eq!(path_for(Some(Path::new("settings.json"))), None);
    }

    #[test]
    fn open_writes_the_template_once_and_never_over_a_file() {
        let dir = scratch_dir("open");
        let state = UserCssState::new(Some(&dir.join("settings.json")), false);
        let file = dir.join(FILE_NAME);
        assert_eq!(state.file_to_open(), Ok(file.clone()));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), TEMPLATE);

        std::fs::write(&file, "mine").unwrap();
        assert_eq!(state.file_to_open(), Ok(file.clone()));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine");

        let gone = UserCssState::new(Some(&dir.join("missing").join("settings.json")), false);
        assert!(
            gone.file_to_open()
                .is_err_and(|e| e.starts_with("could not create ")),
            "a folder that does not exist"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_template_changes_nothing_and_lists_the_stable_variables_of_theming_md() {
        // Every declaration is commented out: the template's CSS, comments
        // removed, is one empty rule.
        let mut css = String::new();
        let mut rest = TEMPLATE;
        while let Some(start) = rest.find("/*") {
            css.push_str(&rest[..start]);
            let end = rest[start + 2..]
                .find("*/")
                .expect("every comment in the template is closed");
            rest = &rest[start + 2 + end + 2..];
        }
        css.push_str(rest);
        let css: String = css.split_whitespace().collect();
        assert_eq!(css, ".dock-stage{}");

        let theming = include_str!("../../docs/THEMING.md");
        assert_eq!(variables(TEMPLATE), variables(theming));
        assert_eq!(variables(TEMPLATE).len(), 9);
        assert!(decode(TEMPLATE.as_bytes().to_vec()).is_ok());
    }

    #[test]
    fn open_tries_the_default_app_then_notepad_and_names_both_failures() {
        let file = Path::new(r"C:\Users\me\AppData\Roaming\dev.davidcristian.stem\dock.css");
        let notepad = notepad_for(file, Some(OsStr::new(r"C:\WINDOWS")));
        assert_eq!(
            notepad,
            LaunchTarget::Exe {
                path: PathBuf::from(r"C:\WINDOWS\System32\notepad.exe"),
                args: vec![file.to_string_lossy().into_owned()],
                working_dir: None,
            }
        );
        let uri = LaunchTarget::Uri {
            uri: file.to_string_lossy().into_owned(),
        };

        // The default app opens it: Notepad is never tried.
        let mut tried = Vec::new();
        let opened = open_file(file, &notepad, |target| {
            tried.push(target.clone());
            Ok(())
        });
        assert_eq!(opened, Ok(Opener::DefaultApp));
        assert_eq!(tried, std::slice::from_ref(&uri));

        // No app for `.css` (the shell refuses without UI): Notepad.
        let mut tried = Vec::new();
        let opened = open_file(file, &notepad, |target| {
            tried.push(target.clone());
            match target {
                LaunchTarget::Uri { .. } => Err("no association".to_owned()),
                _ => Ok(()),
            }
        });
        assert_eq!(opened, Ok(Opener::Notepad));
        assert_eq!(tried, [uri.clone(), notepad.clone()]);

        // Both fail: one error naming the file and both reasons.
        let opened = open_file(file, &notepad, |target| {
            Err(match target {
                LaunchTarget::Uri { .. } => "no association".to_owned(),
                _ => "not found".to_owned(),
            })
        });
        assert_eq!(
            opened,
            Err(format!(
                "could not open {} with its default app (no association) or with Notepad (not found)",
                file.display()
            ))
        );
        assert_eq!(Opener::DefaultApp.to_string(), "with its default app");
        assert_eq!(Opener::Notepad.to_string(), "in Notepad");
    }

    #[test]
    fn notepad_is_under_system_root_when_it_is_known() {
        let file = Path::new(r"C:\Users\me\dock.css");
        let path_of = |root: Option<&str>| match notepad_for(file, root.map(OsStr::new)) {
            LaunchTarget::Exe { path, .. } => path.to_string_lossy().into_owned(),
            other => panic!("not an exe: {other:?}"),
        };
        assert_eq!(
            path_of(Some(r"C:\Windows\")),
            r"C:\Windows\System32\notepad.exe"
        );
        assert_eq!(path_of(Some(r" D:\Win ")), r"D:\Win\System32\notepad.exe");
        assert_eq!(path_of(Some("")), "notepad.exe");
        assert_eq!(path_of(None), "notepad.exe");
    }

    #[test]
    fn a_reload_waits_for_the_one_before_it_to_publish() {
        use std::sync::mpsc;
        use std::sync::{Arc, Mutex as StdMutex};
        use std::time::Duration;

        let dir = scratch_dir("serial");
        let file = dir.join(FILE_NAME);
        let state = Arc::new(UserCssState::new(Some(&dir.join("settings.json")), false));
        let published = Arc::new(StdMutex::new(Vec::<Option<String>>::new()));
        std::fs::write(&file, "old {}").unwrap();

        // The first reload has read "old {}" and is publishing; it waits
        // there until released.
        let (in_publish_tx, in_publish) = mpsc::channel();
        let (release, release_rx) = mpsc::channel::<()>();
        let first = {
            let (state, published) = (Arc::clone(&state), Arc::clone(&published));
            std::thread::spawn(move || {
                state.reload_and(|css| {
                    in_publish_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    published.lock().unwrap().push(css.css.clone());
                })
            })
        };
        in_publish.recv().unwrap();

        // A save and a second Reload meanwhile: it must not read, store or
        // publish before the first has published.
        std::fs::write(&file, "new {}").unwrap();
        let second = {
            let (state, published) = (Arc::clone(&state), Arc::clone(&published));
            std::thread::spawn(move || {
                state.reload_and(|css| published.lock().unwrap().push(css.css.clone()))
            })
        };
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            !second.is_finished(),
            "the second reload ran past the first"
        );
        assert!(published.lock().unwrap().is_empty());

        release.send(()).unwrap();
        assert_eq!(first.join().unwrap().css.as_deref(), Some("old {}"));
        assert_eq!(second.join().unwrap().css.as_deref(), Some("new {}"));
        assert_eq!(
            *published.lock().unwrap(),
            [Some("old {}".to_owned()), Some("new {}".to_owned())]
        );
        assert_eq!(state.current().css.as_deref(), Some("new {}"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_event_goes_to_dock_windows_by_label() {
        for label in ["dock", "dock-DISPLAY3"] {
            assert!(is_dock_target(&EventTarget::WebviewWindow {
                label: label.to_owned()
            }));
            assert!(is_dock_target(&EventTarget::AnyLabel {
                label: label.to_owned()
            }));
        }
        assert!(!is_dock_target(&EventTarget::WebviewWindow {
            label: "settings".to_owned()
        }));
        assert!(!is_dock_target(&EventTarget::App));
        assert!(!is_dock_target(&EventTarget::Any));
    }
}
