//! The `winuser.h` WinEvent and shell hook codes, and the WinEvent ranges
//! the hooks are installed for.

pub const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
pub const EVENT_SYSTEM_MINIMIZESTART: u32 = 0x0016;
pub const EVENT_SYSTEM_MINIMIZEEND: u32 = 0x0017;
pub const EVENT_OBJECT_CREATE: u32 = 0x8000;
pub const EVENT_OBJECT_DESTROY: u32 = 0x8001;
pub const EVENT_OBJECT_SHOW: u32 = 0x8002;
pub const EVENT_OBJECT_HIDE: u32 = 0x8003;
/// Hooked on its own, with a callback that keeps only moves
/// and resizes of top-level windows ([`location_change_tracked`]) and a loop
/// that coalesces them; it is not in the translation table and never reaches
/// [`event_intent`].
///
/// [`location_change_tracked`]: super::location_change_tracked
/// [`event_intent`]: super::event_intent
pub const EVENT_OBJECT_LOCATIONCHANGE: u32 = 0x800B;
pub const EVENT_OBJECT_NAMECHANGE: u32 = 0x800C;
pub const EVENT_OBJECT_CLOAKED: u32 = 0x8017;
pub const EVENT_OBJECT_UNCLOAKED: u32 = 0x8018;

/// `idObject` value for the window itself.
pub const OBJID_WINDOW: i32 = 0;
/// `idChild` value for the object itself (not one of its children).
pub const CHILDID_SELF: i32 = 0;

pub const HSHELL_WINDOWCREATED: u32 = 1;
pub const HSHELL_WINDOWDESTROYED: u32 = 2;
pub const HSHELL_ACTIVATESHELLWINDOW: u32 = 3;
pub const HSHELL_WINDOWACTIVATED: u32 = 4;
pub const HSHELL_GETMINRECT: u32 = 5;
pub const HSHELL_REDRAW: u32 = 6;
pub const HSHELL_TASKMAN: u32 = 7;
pub const HSHELL_LANGUAGE: u32 = 8;
pub const HSHELL_SYSMENU: u32 = 9;
pub const HSHELL_ENDTASK: u32 = 10;
pub const HSHELL_ACCESSIBILITYSTATE: u32 = 11;
pub const HSHELL_APPCOMMAND: u32 = 12;
pub const HSHELL_WINDOWREPLACED: u32 = 13;
pub const HSHELL_WINDOWREPLACING: u32 = 14;
pub const HSHELL_MONITORCHANGED: u32 = 16;
/// Set on `HSHELL_RUDEAPPACTIVATED` and `HSHELL_FLASH`.
pub const HSHELL_HIGHBIT: u32 = 0x8000;
pub const HSHELL_RUDEAPPACTIVATED: u32 = HSHELL_WINDOWACTIVATED | HSHELL_HIGHBIT;
pub const HSHELL_FLASH: u32 = HSHELL_REDRAW | HSHELL_HIGHBIT;

/// `(eventMin, eventMax)` pairs for `SetWinEventHook`, covering exactly the
/// events in the translation table and nothing else.
pub const HOOK_RANGES: &[(u32, u32)] = &[
    (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
    (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
    (EVENT_OBJECT_CREATE, EVENT_OBJECT_HIDE),
    (EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE),
    (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
];
