//! The translation table: what the event thread does about each WinEvent
//! and shell hook message, and the names of both for log lines.

use super::codes::*;

/// What the event thread should do about an OS event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Query the window; emit `WindowAdded` if it qualifies. A show-type
    /// event: a transient rejection schedules re-checks (`rules::recheck`).
    Added,
    /// Emit `WindowRemoved` if the window was known.
    Removed,
    /// Re-query the window; emit `WindowUpdated` (or `WindowRemoved`).
    Updated,
    /// Like `Updated`, but coalesced through the [`TitleDebouncer`].
    ///
    /// [`TitleDebouncer`]: super::TitleDebouncer
    TitleChanged,
    /// Emit `ForegroundChanged`.
    Foreground,
    /// Emit `WindowAttention`.
    Attention,
    /// Nothing to do.
    Ignore,
}

/// Whether the hook callback could establish that the window is top-level
/// (`GetAncestor(GA_ROOT) == hwnd`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopLevel {
    Yes,
    No,
    /// The window is already gone (`GetAncestor` returned NULL), which is
    /// normal for `EVENT_OBJECT_DESTROY` delivered out of context.
    Unknown,
}

/// The event-id column of the translation table, without the filters.
pub fn event_intent(event: u32) -> Intent {
    match event {
        EVENT_OBJECT_CREATE | EVENT_OBJECT_SHOW => Intent::Added,
        EVENT_OBJECT_DESTROY | EVENT_OBJECT_HIDE => Intent::Removed,
        EVENT_OBJECT_NAMECHANGE => Intent::TitleChanged,
        EVENT_SYSTEM_MINIMIZESTART
        | EVENT_SYSTEM_MINIMIZEEND
        | EVENT_OBJECT_CLOAKED
        | EVENT_OBJECT_UNCLOAKED => Intent::Updated,
        EVENT_SYSTEM_FOREGROUND => Intent::Foreground,
        _ => Intent::Ignore,
    }
}

/// The name of a hooked event, for log lines.
pub fn event_name(event: u32) -> &'static str {
    match event {
        EVENT_SYSTEM_FOREGROUND => "EVENT_SYSTEM_FOREGROUND",
        EVENT_SYSTEM_MINIMIZESTART => "EVENT_SYSTEM_MINIMIZESTART",
        EVENT_SYSTEM_MINIMIZEEND => "EVENT_SYSTEM_MINIMIZEEND",
        EVENT_OBJECT_CREATE => "EVENT_OBJECT_CREATE",
        EVENT_OBJECT_DESTROY => "EVENT_OBJECT_DESTROY",
        EVENT_OBJECT_SHOW => "EVENT_OBJECT_SHOW",
        EVENT_OBJECT_HIDE => "EVENT_OBJECT_HIDE",
        EVENT_OBJECT_NAMECHANGE => "EVENT_OBJECT_NAMECHANGE",
        EVENT_OBJECT_CLOAKED => "EVENT_OBJECT_CLOAKED",
        EVENT_OBJECT_UNCLOAKED => "EVENT_OBJECT_UNCLOAKED",
        _ => "an unhooked event",
    }
}

/// Filter + translate a `WINEVENTPROC` callback. Returns [`Intent::Ignore`]
/// for anything that is not about a top-level window as a whole. A window
/// whose top-level status is [`TopLevel::Unknown`] is accepted only for
/// removals: it has already been destroyed, so it can only ever be forgotten.
pub fn win_event_intent(event: u32, id_object: i32, id_child: i32, top_level: TopLevel) -> Intent {
    if id_object != OBJID_WINDOW || id_child != CHILDID_SELF {
        return Intent::Ignore;
    }
    let intent = event_intent(event);
    match top_level {
        TopLevel::Yes => intent,
        TopLevel::Unknown if intent == Intent::Removed => intent,
        TopLevel::No | TopLevel::Unknown => Intent::Ignore,
    }
}

/// Filter of the `EVENT_OBJECT_LOCATIONCHANGE` hook's callback:
/// whether a report is about a top-level window as a whole moving or
/// resizing, and so worth a thread message. Caret and cursor moves
/// (`OBJID_CARET`, `OBJID_CURSOR`), child controls and a window that is
/// already gone are dropped in the callback, before they cost anything.
pub fn location_change_tracked(id_object: i32, id_child: i32, top_level: TopLevel) -> bool {
    id_object == OBJID_WINDOW && id_child == CHILDID_SELF && top_level == TopLevel::Yes
}

/// Translate the `wParam` of a `RegisterShellHookWindow` message.
pub fn shell_hook_intent(code: u32) -> Intent {
    match code {
        HSHELL_WINDOWCREATED => Intent::Added,
        HSHELL_WINDOWACTIVATED | HSHELL_RUDEAPPACTIVATED => Intent::Foreground,
        HSHELL_FLASH => Intent::Attention,
        _ => Intent::Ignore,
    }
}

/// The name of a shell hook code (every one `winuser.h` documents), for the
/// one trace line per message that lets a trace log be counted per source.
pub fn shell_hook_name(code: u32) -> &'static str {
    match code {
        HSHELL_WINDOWCREATED => "HSHELL_WINDOWCREATED",
        HSHELL_WINDOWDESTROYED => "HSHELL_WINDOWDESTROYED",
        HSHELL_ACTIVATESHELLWINDOW => "HSHELL_ACTIVATESHELLWINDOW",
        HSHELL_WINDOWACTIVATED => "HSHELL_WINDOWACTIVATED",
        HSHELL_GETMINRECT => "HSHELL_GETMINRECT",
        HSHELL_REDRAW => "HSHELL_REDRAW",
        HSHELL_TASKMAN => "HSHELL_TASKMAN",
        HSHELL_LANGUAGE => "HSHELL_LANGUAGE",
        HSHELL_SYSMENU => "HSHELL_SYSMENU",
        HSHELL_ENDTASK => "HSHELL_ENDTASK",
        HSHELL_ACCESSIBILITYSTATE => "HSHELL_ACCESSIBILITYSTATE",
        HSHELL_APPCOMMAND => "HSHELL_APPCOMMAND",
        HSHELL_WINDOWREPLACED => "HSHELL_WINDOWREPLACED",
        HSHELL_WINDOWREPLACING => "HSHELL_WINDOWREPLACING",
        HSHELL_MONITORCHANGED => "HSHELL_MONITORCHANGED",
        HSHELL_RUDEAPPACTIVATED => "HSHELL_RUDEAPPACTIVATED",
        HSHELL_FLASH => "HSHELL_FLASH",
        _ => "an unknown shell hook code",
    }
}
