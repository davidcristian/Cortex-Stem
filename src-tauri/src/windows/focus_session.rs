//! The focus shortcut's session (`docs/ipc/keyboard-entry.md`): its
//! rules are methods of [`DockFocus`] that touch nothing but its lock.

use parking_lot::Mutex;
use stem_core::WindowId;

/// The focus shortcut's session (`docs/ipc/keyboard-entry.md`):
/// the dock window the latest press gave the keyboard focus to, and the
/// window that was in front then, which Escape or a second press
/// (`release_dock_focus`) brings back. `None` while no dock has the
/// keyboard from the shortcut: before the first press, and again once the
/// focus was given back ([`DockFocus::release`]), the dock lost the
/// activation ([`DockFocus::blurred`]), it closed or its activation failed
/// ([`DockFocus::end`]). A click on a dock forgets only the window
/// ([`DockFocus::supersede`]): the dock may still have the keyboard, and
/// then a second press still lets go ([`DockFocus::holder`]). Kept in
/// Tauri's managed state. The rules are its methods, which touch nothing
/// but the lock, so they are tested without an app; the functions in
/// `windows/focus_shortcut.rs` and `windows/focus_release.rs` that take an
/// `AppHandle` only gather their inputs and log.
#[derive(Debug, Default)]
pub struct DockFocus(Mutex<Option<FocusSession>>);

/// One press of the focus shortcut, as [`DockFocus`] keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusSession {
    /// The label of the dock window that was given the focus.
    pub dock: String,
    /// The window that was in front at the press ([`return_target`]);
    /// `None` when there was none to go back to (the desktop or the Start
    /// menu was in front) or a click on a dock superseded it, and then
    /// Escape leaves the foreground alone.
    ///
    /// [`return_target`]: super::focus_target::return_target
    pub window: Option<ReturnTo>,
}

/// The window a press of the focus shortcut gives the keyboard focus back
/// to ([`return_target`]).
///
/// [`return_target`]: super::focus_target::return_target
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReturnTo {
    /// An app's window: the engine's active window at the press
    /// (`DockState.foreground`). Brought back with `Activate`
    /// ([`return_focus`]).
    ///
    /// [`return_focus`]: super::focus_release::return_focus
    App(WindowId),
    /// One of Stem's own windows other than a dock window (the settings
    /// window), by its label. The engine never takes Stem's own windows for
    /// the foreground, so its active window is then the app behind it.
    /// Brought back with Tauri's `set_focus`.
    Own(String),
}

/// What the dock window that gives the keyboard focus back
/// ([`DockFocus::release`]) is to bring back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Release {
    /// The session was this dock's: bring this window back.
    Window(ReturnTo),
    /// The session was this dock's, with nothing to go back to (nothing was
    /// in front at the press, or a click superseded the window): the
    /// foreground stays.
    Nothing,
    /// The session was another dock window's (this label), which is no
    /// longer the foreground window and lost the activation without its
    /// blur reaching us: stale, and dropped too; the foreground stays.
    Stale(String),
    /// The session is another dock window's (this label), which is still
    /// the foreground window: it still has the keyboard and its own release
    /// brings the window back, so the session stays; nothing happens here.
    Held(String),
    /// No session: the focus shortcut did not bring the keyboard here.
    NoSession,
}

impl DockFocus {
    /// The dock window a press of the focus shortcut lets go of, as Escape
    /// does: `front` (the dock window that is the foreground window, if one
    /// is) when the session is that dock's, which still has the keyboard
    /// from an earlier press. `None`: the press gives a dock the focus, also
    /// while a dock is in front without the session (after an Escape that
    /// had nothing to go back to).
    pub fn holder(&self, front: Option<&str>) -> Option<String> {
        let front = front?;
        self.0
            .lock()
            .as_ref()
            .filter(|session| session.dock == front)
            .map(|session| session.dock.clone())
    }

    /// A press gives the dock window `dock` the focus; `window` is the
    /// window in front at the press ([`return_target`]). Replaces any
    /// earlier session.
    ///
    /// [`return_target`]: super::focus_target::return_target
    pub fn begin(&self, dock: &str, window: Option<ReturnTo>) {
        *self.0.lock() = Some(FocusSession {
            dock: dock.to_owned(),
            window,
        });
    }

    /// The dock window `label` gives the keyboard focus back (Escape, or a
    /// second press) while `front` is the dock window that is the
    /// foreground window (`None`: no dock is): take the session and say
    /// what to bring back. A session of another dock that is still in front
    /// is that dock's to give back and stays ([`Release::Held`]: a stray
    /// release from a dock that does not have the keyboard cannot lose the
    /// window to go back to); one whose dock is no longer in front is stale
    /// and is dropped too.
    pub fn release(&self, label: &str, front: Option<&str>) -> Release {
        let mut guard = self.0.lock();
        match guard.as_ref() {
            None => return Release::NoSession,
            Some(session) if session.dock != label && front == Some(session.dock.as_str()) => {
                return Release::Held(session.dock.clone());
            }
            Some(_) => {}
        }
        match guard.take() {
            Some(FocusSession {
                dock,
                window: Some(window),
            }) if dock == label => Release::Window(window),
            Some(session) if session.dock == label => Release::Nothing,
            Some(session) => Release::Stale(session.dock),
            None => Release::NoSession,
        }
    }

    /// A click on a dock (`primary_click`, `launch_app`, `window_action`)
    /// activates or launches something, which supersedes the window the
    /// focus shortcut remembered: forget it, whichever dock holds the
    /// session, so a later Escape does not undo the click. The session
    /// stays: when the click leaves that dock in front (a launch whose
    /// window is not up yet, a minimise) it still has the keyboard, and a
    /// second press still lets go; when it brings another window to the
    /// front, the dock's blur ends the session. The dock whose window was
    /// forgotten, if one was.
    pub fn supersede(&self) -> Option<String> {
        let mut guard = self.0.lock();
        let session = guard.as_mut()?;
        session.window.take().map(|_| session.dock.clone())
    }

    /// End the session if it is the dock window `label`'s (it closed, or
    /// its activation failed): the session that ended, if one did, whose
    /// `window` says whether there was still one to go back to
    /// ([`end_outcome`]).
    pub fn end(&self, label: &str) -> Option<FocusSession> {
        let mut session = self.0.lock();
        if session.as_ref().is_some_and(|s| s.dock == label) {
            session.take()
        } else {
            None
        }
    }

    /// The dock window `label`'s webview lost the keyboard focus (Tauri's
    /// `Focused(false)`) while `front` is the dock window that is the
    /// foreground window (`None`: no dock is). When `label` is not in front
    /// any more, another window took the activation (a click elsewhere,
    /// Alt+Tab, the window Escape brought back) and its session ends; a dock
    /// that is still in front (its own native menu took the focus for a
    /// moment) keeps it. The session that ended, if one did: its `window`
    /// is `None` when there was nothing to go back to at the press or a
    /// click or a menu entry already forgot it ([`DockFocus::supersede`]).
    pub fn blurred(&self, label: &str, front: Option<&str>) -> Option<FocusSession> {
        if front == Some(label) {
            return None;
        }
        self.end(label)
    }

    /// The session as it is now (for the tests).
    #[cfg(test)]
    pub(super) fn session(&self) -> Option<FocusSession> {
        self.0.lock().clone()
    }
}

/// What the end of `session` forgot ([`DockFocus::end`], by a blur
/// [`DockFocus::blurred`], a closed dock window or a failed activation),
/// for its log line: the window to go back to while there still was one,
/// else (nothing was in front at the press, or a click or a menu entry
/// forgot it already, with a line of its own) only the keyboard session.
/// Pure.
pub(super) fn end_outcome(session: &FocusSession) -> &'static str {
    if session.window.is_some() {
        "the window to go back to is forgotten"
    } else {
        "the keyboard session ended"
    }
}
