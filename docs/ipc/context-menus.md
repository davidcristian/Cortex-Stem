# Context menus

Right-click menus are **native** (Tauri `Menu::popup_at`). The UI reports only *what* was
right-clicked and *where*. Rust builds the menu from its current snapshot and settings, shows it,
and runs the chosen entry itself. The UI never receives the choice: its effect arrives as the
usual `dock://state` and `dock://settings` events (a window activated, an app pinned) or as the
settings window opening.

- `x` and `y` are CSS pixels relative to the **calling dock window's viewport**
  (`MouseEvent.clientX/Y`). Rust multiplies them by the window's scale factor. Only dock windows
  may call; non-finite numbers reject.
- `show_item_menu` with a separator key shows nothing and resolves; an unknown key rejects
  (`unknown dock item: <key>`).
- Contents by item kind (`DockItem.kind`):
  - `app`: one entry per running window. The entry is the window's title, trimmed and cut to 60
    characters with `…` appended when it is longer, and it activates the window. Then come a
    separator if there were any windows; *New window* (`launch_app`, only when the app can be
    launched: pinned, or an `exe:` or `aumid:` id); *Pin to Stem* or *Unpin from Stem*
    (`pin_app` / `unpin_app`; *Pin to Stem* is disabled for apps without a launch target, that
    is `class:` ids); and *Close all windows* when the app has windows (`window_action(Close)`
    for each).
  - `launcher`: *Open Start menu*; `taskView`: *Open Task View*; `showDesktop`: *Show desktop*;
    `recycleBin`: *Open*, *Empty Recycle Bin…* (all through `shell_action`).
  - Every item menu ends with a separator, *Stem settings…* (`open_settings`) and *Quit Stem*
    (`quit`).
- `show_dock_menu`: *Stem settings…*, a separator, *Quit Stem*.
- Rust builds each menu when it is shown, so the pinned state and the window lists are current
  each time a menu opens. A chosen entry that fails (the window is gone, the app is not
  launchable) is logged at `warn`; there is no error path back to the UI.
- `MockBackend` has no native menus: it records the request (`calls` entries `showItemMenu` and
  `showDockMenu`) and logs it.
