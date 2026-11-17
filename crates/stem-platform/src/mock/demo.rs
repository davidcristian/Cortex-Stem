//! The demo scenario: one monitor, a laptop's battery, an output device and
//! a handful of windows.

use std::path::PathBuf;

use stem_core::{MonitorId, MonitorInfo, PowerStatus, RawWindow, Rect, VolumeStatus, WindowId};

use super::identity::file_stem;

pub(super) fn demo_monitor() -> MonitorInfo {
    MonitorInfo {
        id: MonitorId("mock-primary".into()),
        name: "Mock Display".into(),
        bounds: Rect::new(0, 0, 2560, 1440),
        work_area: Rect::new(0, 0, 2560, 1392),
        scale_factor: 1.0,
        is_primary: true,
    }
}

/// The demo's power reading: a laptop at 76 %, on battery. The TS
/// mirror (`MockOptions.power` in `ui/src/lib/ipc/mock/options.ts`) defaults to
/// the same.
pub(super) fn demo_power() -> PowerStatus {
    PowerStatus {
        has_battery: true,
        percent: Some(76),
        charging: false,
        on_ac: false,
        saver: false,
    }
}

/// The demo's output device: at 60 %, not muted. The TS mirror
/// (`MockOptions.volume` in `ui/src/lib/ipc/mock/options.ts`) defaults to the
/// same.
pub(super) fn demo_volume() -> VolumeStatus {
    VolumeStatus {
        percent: 60,
        muted: false,
    }
}

pub(super) fn demo_window(
    id: u64,
    exe: &str,
    title: &str,
    minimized: bool,
    rect: Rect,
) -> RawWindow {
    RawWindow {
        id: WindowId(id),
        title: title.into(),
        class_name: format!("{}Class", file_stem(std::path::Path::new(exe))),
        process_id: 1000 + id as u32,
        exe_path: Some(PathBuf::from(exe)),
        app_user_model_id: None,
        is_visible: true,
        is_minimized: minimized,
        is_cloaked: false,
        on_current_desktop: Some(true),
        monitor: Some(MonitorId("mock-primary".into())),
        rect: Some(rect),
        icon_key: None,
    }
}

/// The demo's windows, each with a plausible frame on the demo monitor:
/// all of them keep clear of the 250 px along the bottom, left and
/// right edges of the work area, where the dock's panel rests on those
/// edges, so nothing overlaps it until a test moves one
/// (`move_window`). The top band is not kept clear (the frames start near
/// the top, as on a real desktop). A minimised window keeps the frame it
/// would be restored to; the overlap rule ignores it while it is minimised.
/// The TS mirror (`ui/src/lib/ipc/mock/scenarios.ts`) uses the same frames.
pub(super) fn demo_windows() -> Vec<RawWindow> {
    vec![
        demo_window(
            1,
            r"C:\Windows\explorer.exe",
            "Downloads",
            false,
            Rect::new(260, 90, 1100, 700),
        ),
        demo_window(
            2,
            r"C:\Program Files\Mozilla Firefox\firefox.exe",
            "Stem — Mozilla Firefox",
            false,
            Rect::new(640, 60, 1500, 1000),
        ),
        demo_window(
            3,
            r"C:\Program Files\Mozilla Firefox\firefox.exe",
            "GitHub — Mozilla Firefox",
            true,
            Rect::new(700, 120, 1400, 900),
        ),
        demo_window(
            4,
            r"C:\Users\me\AppData\Local\Programs\Microsoft VS Code\Code.exe",
            "lib.rs — Stem",
            false,
            Rect::new(260, 40, 1600, 1000),
        ),
        demo_window(
            5,
            r"C:\Program Files\WindowsApps\Terminal\WindowsTerminal.exe",
            "PowerShell",
            false,
            Rect::new(1300, 500, 1000, 600),
        ),
        demo_window(
            6,
            r"C:\Program Files\Spotify\Spotify.exe",
            "Spotify Premium",
            true,
            Rect::new(400, 200, 1200, 800),
        ),
    ]
}
