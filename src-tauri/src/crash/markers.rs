//! The two markers and what the previous run's leftovers mean.

/// The dialog shows at most this much of the panic message; the rest is in
/// the log and in the marker.
pub(super) const DIALOG_MESSAGE_CHARS: usize = 400;

/// `run.json`: the run that is (or was) in progress.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMarker {
    /// RFC 3339, UTC.
    pub started_at: String,
    pub pid: u32,
    /// The package version of the binary that wrote it.
    pub version: String,
}

/// `crash.json`: what the panic hook saw.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashMarker {
    pub kind: CrashKind,
    pub message: String,
    /// `file:line:column`, as the panic hook formats it.
    pub location: String,
    /// RFC 3339, UTC.
    pub time: String,
}

/// What kind of crash a marker records. Only panics are caught today; the
/// field exists so that another kind can be told apart later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CrashKind {
    Panic,
}

/// What the previous run left behind means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing to report: a clean exit, or the first start.
    Clean,
    /// The previous run never reached `RunEvent::Exit` and did not panic:
    /// log once.
    UncleanExit(RunMarker),
    /// The previous run panicked: tell the user once.
    Crashed(CrashMarker),
}

/// The decision, pure: a crash marker wins; a run marker from another
/// process is an unclean exit; a run marker with this process's pid cannot
/// be a previous run's (this run has not written one yet) and is ignored.
pub fn assess(crash: Option<CrashMarker>, run: Option<RunMarker>, this_pid: u32) -> Verdict {
    if let Some(crash) = crash {
        return Verdict::Crashed(crash);
    }
    match run {
        Some(run) if run.pid != this_pid => Verdict::UncleanExit(run),
        _ => Verdict::Clean,
    }
}

/// The dialog's text for a crash: when, where, what, and the question. The
/// message is cut at [`DIALOG_MESSAGE_CHARS`] characters (on a character
/// boundary) with an ellipsis, so a long panic message cannot produce a
/// screen-high box.
pub fn dialog_text(crash: &CrashMarker) -> String {
    let message = match crash.message.char_indices().nth(DIALOG_MESSAGE_CHARS) {
        Some((cut, _)) => format!("{}…", &crash.message[..cut]),
        None => crash.message.clone(),
    };
    format!(
        "Stem crashed last time ({}, {}):\n{message}\n\nOpen the log folder?",
        crash.time, crash.location
    )
}
