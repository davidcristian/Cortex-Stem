//! Reading `dock.css`, bounded by its size, and what the log and the
//! page are told about it.

use std::fs::File;
use std::io::{ErrorKind, Read as _};
use std::path::Path;

use stem_core::UserCssProblem;

use super::MAX_BYTES;

/// Why a file's text is not used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Larger than [`MAX_BYTES`]: the size seen (at least `MAX_BYTES + 1`).
    TooLarge(u64),
    NotUtf8,
    /// Opening or reading failed, or the path is not a file.
    Unreadable(String),
}

impl Refusal {
    /// What the page is told about it ([`UserCss::problem`]); the details
    /// (the size, the OS error) are the log's.
    ///
    /// [`UserCss::problem`]: stem_core::UserCss::problem
    pub fn problem(&self) -> UserCssProblem {
        match self {
            Self::TooLarge(_) => UserCssProblem::TooLarge,
            Self::NotUtf8 => UserCssProblem::NotUtf8,
            Self::Unreadable(_) => UserCssProblem::Unreadable,
        }
    }
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
/// what the page gets: the CSS, or why there is none.
pub(super) fn report(path: &Path, outcome: ReadOutcome) -> Result<String, UserCssProblem> {
    match outcome {
        ReadOutcome::Missing => {
            log::info!(
                "dock.css: none at {}; the dock uses its own styles only",
                path.display()
            );
            Err(UserCssProblem::NotFound)
        }
        ReadOutcome::Css(css) => {
            log::info!("dock.css: {} bytes from {}", css.len(), path.display());
            Ok(css)
        }
        ReadOutcome::Refused(refusal) => {
            let problem = refusal.problem();
            match refusal {
                Refusal::TooLarge(size) => log::warn!(
                    "dock.css at {} is ignored: it is {size} bytes, more than the {MAX_BYTES} (64 KiB) Stem reads",
                    path.display()
                ),
                Refusal::NotUtf8 => log::warn!(
                    "dock.css at {} is ignored: it is not UTF-8 text (save it as UTF-8)",
                    path.display()
                ),
                Refusal::Unreadable(e) => log::warn!(
                    "dock.css at {} is ignored: it cannot be read ({e})",
                    path.display()
                ),
            }
            Err(problem)
        }
    }
}
