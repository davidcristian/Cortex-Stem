//! What the grid holds: the sources the probe creates or finds, and one
//! plan per slot.

use windows::Win32::Foundation::COLORREF;

use super::rgb;

/// A source the probe creates itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Own {
    Plain,
    Uncropped,
    Minimised,
    Cloaked,
    ExcludeFromCapture,
    MonitorOnly,
}

/// A source the probe can only find.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Find {
    Elevated,
    StoreApp,
    OtherDesktop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
    Own(Own),
    Find(Find),
    /// The source of slot 1 again (phase 4, A11).
    SecondOfFirst,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Plan {
    pub(super) key: &'static str,
    pub(super) what: &'static str,
    pub(super) answers: &'static str,
    pub(super) source: Source,
    /// Crop `rcSource` to the extended frame bounds.
    pub(super) crop: bool,
}

pub(super) const PLANS: [Plan; 10] = [
    Plan {
        key: "normal",
        what: "own window, cropped to its frame",
        answers: "A1 A2 A7",
        source: Source::Own(Own::Plain),
        crop: true,
    },
    Plan {
        key: "no crop",
        what: "own window, whole source",
        answers: "A7 A6",
        source: Source::Own(Own::Uncropped),
        crop: false,
    },
    Plan {
        key: "minimised",
        what: "own window, minimised",
        answers: "A3",
        source: Source::Own(Own::Minimised),
        crop: true,
    },
    Plan {
        key: "cloaked",
        what: "own window, DWMWA_CLOAK",
        answers: "A4",
        source: Source::Own(Own::Cloaked),
        crop: true,
    },
    Plan {
        key: "exclude",
        what: "own window, WDA_EXCLUDEFROMCAPTURE",
        answers: "A9",
        source: Source::Own(Own::ExcludeFromCapture),
        crop: true,
    },
    Plan {
        key: "monitor",
        what: "own window, WDA_MONITOR",
        answers: "A9",
        source: Source::Own(Own::MonitorOnly),
        crop: true,
    },
    Plan {
        key: "elevated",
        what: "Task Manager, elevated",
        answers: "A8",
        source: Source::Find(Find::Elevated),
        crop: true,
    },
    Plan {
        key: "store app",
        what: "Calculator, minimised or suspended",
        answers: "A4",
        source: Source::Find(Find::StoreApp),
        crop: true,
    },
    Plan {
        key: "other desktop",
        what: "a window on another virtual desktop",
        answers: "A4",
        source: Source::Find(Find::OtherDesktop),
        crop: true,
    },
    Plan {
        key: "second",
        what: "slot 1's source again (phase 4)",
        answers: "A11",
        source: Source::SecondOfFirst,
        crop: true,
    },
];

pub(super) const fn own_colour(kind: Own) -> COLORREF {
    match kind {
        Own::Plain => rgb(0x2E, 0x7D, 0x32),
        Own::Uncropped => rgb(0x15, 0x65, 0xC0),
        Own::Minimised => rgb(0x6A, 0x1B, 0x9A),
        Own::Cloaked => rgb(0xC6, 0x28, 0x28),
        Own::ExcludeFromCapture => rgb(0xE6, 0x51, 0x00),
        Own::MonitorOnly => rgb(0x00, 0x83, 0x8F),
    }
}

pub(super) const fn missing_hint(kind: Find) -> &'static str {
    match kind {
        Find::Elevated => "not found: open Task Manager (Ctrl+Shift+Esc) before the probe",
        Find::StoreApp => "not found: open Calculator and minimise it before the probe",
        Find::OtherDesktop => {
            "not found: open Notepad on a second virtual desktop (Win+Ctrl+D), then come back"
        }
    }
}
