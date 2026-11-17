//! A realistic sample settings document.

use super::{PinnedApp, Settings};
use crate::model::{AppId, LaunchTarget};

impl Settings {
    /// A realistic, fully valid document: a few pinned apps that use every
    /// [`LaunchTarget`] kind, and a handful of non-default options. The
    /// tests in `settings/tests.rs` check that it validates unchanged and
    /// survives a round trip through JSON.
    pub fn example() -> Self {
        use std::path::PathBuf;

        let firefox = PathBuf::from(r"C:\Program Files\Mozilla Firefox\firefox.exe");
        let code = PathBuf::from(r"C:\Users\me\AppData\Local\Programs\Microsoft VS Code\Code.exe");
        let steam_exe = PathBuf::from(r"C:\Program Files (x86)\Steam\steam.exe");
        let calculator = "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App";
        let settings_app =
            "windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel";

        let mut example = Self {
            icon_size: 56,
            ..Self::default()
        };
        example.magnification.max_scale = 1.8;
        example.items.task_view = true;
        example.appearance.corner_radius = 24;
        example.pinned = vec![
            PinnedApp {
                id: AppId::for_exe(&firefox),
                label: None,
                launch: LaunchTarget::Exe {
                    path: firefox,
                    args: vec![],
                    working_dir: None,
                },
            },
            PinnedApp {
                id: AppId::for_exe(&code),
                label: Some("VS Code".into()),
                launch: LaunchTarget::Exe {
                    path: code,
                    args: vec!["--new-window".into()],
                    working_dir: Some(PathBuf::from(r"C:\Users\me\Projects")),
                },
            },
            // A shortcut is pinned under the id of the executable it resolves
            // to, so the running windows merge into the same item.
            PinnedApp {
                id: AppId::for_exe(&steam_exe),
                label: None,
                launch: LaunchTarget::Shortcut {
                    path: PathBuf::from(
                        r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Steam\Steam.lnk",
                    ),
                },
            },
            PinnedApp {
                id: AppId::for_app_user_model_id(calculator),
                label: None,
                launch: LaunchTarget::StoreApp {
                    app_user_model_id: calculator.into(),
                },
            },
            // The Settings app is launched by URI, but its windows have the
            // immersive control panel's AppUserModelID.
            PinnedApp {
                id: AppId::for_app_user_model_id(settings_app),
                label: Some("Settings".into()),
                launch: LaunchTarget::Uri {
                    uri: "ms-settings:".into(),
                },
            },
        ];
        example
    }
}
