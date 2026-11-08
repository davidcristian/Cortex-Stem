# The settings window

Open the settings window from the tray icon (_Settings…_) or from the dock
(right-click, then _Stem settings…_). Every change applies at once and is
saved at once. There is no _Apply_ button. _Reset to defaults_ at the bottom
of the window shows a confirmation first. Its _Reset_ button restores every
default and removes all pinned apps.

The settings are stored in one file,
`%APPDATA%\dev.davidcristian.stem\settings.json`. The _Key_ column of each
table below is the setting's name in that file. Stem reads the file when it
starts and writes it again after every change, so quit Stem before you edit
the file by hand. A number outside its range is set to the nearest end of
the range. A file that Stem cannot read is renamed to `settings.json.broken`,
and Stem starts with the defaults. Such a file is broken JSON, or a file
written by a newer version of Stem.

Sizes are in CSS pixels, which Windows display scaling multiplies: at 150 %
scaling, an icon size of 48 is drawn 72 physical pixels wide.

<details>
<summary>The settings window, shown with demo data</summary>

![The settings window in the dark theme, at the top of its Appearance section](../images/readme-settings.png)

</details>

## Appearance

| Setting          | Key                         | Default       | What it does |
| ---------------- | --------------------------- | ------------- | ------------ |
| Icon size        | `iconSize`                  | `48`          | The size of the icons at rest, 24 to 128. |
| Magnification    | `magnification.enabled`     | on            | Makes icons grow as the pointer approaches them. |
| Maximum scale    | `magnification.maxScale`    | `1.6`         | The scale that the icon right under the pointer grows to, 1.0 to 2.5. |
| Range            | `magnification.rangeIcons`  | `3`           | The number of neighbouring icons on each side that grow too, 1 to 6. |
| Panel opacity    | `appearance.opacity`        | `0.72` (72 %) | The opacity of the panel's background, 0 to 1. |
| Corner radius    | `appearance.cornerRadius`   | `22`          | The radius of the panel's corners, 0 to 64. |
| Backdrop         | `appearance.backdrop`       | `css`         | What is drawn behind the panel. Only _Blur (built in)_ (`css`) can be chosen: _Acrylic (not yet available)_ and _Mica (not yet available)_ are greyed out. Stem keeps `acrylic` or `mica` if the file has it, but the panel still uses the blur. |
| Tooltips         | `appearance.tooltips`       | on            | Shows the app's name above an icon when the pointer is over it. |
| Bounce on launch | `appearance.bounceOnLaunch` | on            | Bounces the icon of an app you started from the dock while the app is starting. |
| Theme            | `theme`                     | `system`      | Sets the colours of the dock and the settings window. _Use Windows setting_ (`system`) uses the Windows app mode (light or dark). _Light_ (`light`) and _Dark_ (`dark`) override it. |

In every theme, the dock uses the Windows accent colour. With _Transparency
effects_ off in Windows, the panel is almost opaque. With a Windows contrast
theme on, the dock and the settings window use the contrast theme's solid
colours.

## Behaviour

| Setting                                  | Key                               | Default      | What it does |
| ---------------------------------------- | --------------------------------- | ------------ | ------------ |
| Clicking the active app minimises it     | `behavior.clickActiveMinimizes`   | on           | A click on an app whose only window is in front minimises that window. See [Clicking an app](clicking-an-app.md). |
| Show only windows on the current desktop | `behavior.showOnlyCurrentDesktop` | on           | Leaves out windows on other virtual desktops. Pinned apps stay on the dock. |
| Show only windows on the dock's monitor  | `behavior.showOnlyCurrentMonitor` | off          | With a dock on every monitor, each dock lists only the windows on its own monitor. See [More than one monitor](multiple-monitors.md). |
| Running indicators                       | `behavior.runningIndicators`      | on           | Draws the dots that show an app's open windows. See [Clicking an app](clicking-an-app.md). |
| Click through outside the dock           | `behavior.clickThrough`           | on           | Clicks beside and above the panel reach the desktop and the windows behind it. With it off, the whole dock window takes clicks, and that window spans the whole edge of the screen. The switch is there for diagnosing problems, not as a preference. It is also a check item in the tray icon's menu. |
| Toggle the dock                          | `behavior.toggleShortcut`         | `Ctrl+Alt+D` | A keyboard shortcut that shows or hides the dock from any app. An empty value means no shortcut. See [The toggle shortcut](keyboard-shortcuts.md#the-toggle-shortcut). |
| Auto-hide                                | `autoHide.mode`                   | `off`        | _Off_ (`off`), _Always_ (`always`) or _When a window overlaps_ (`whenOverlapped`). See [Auto-hide](auto-hide.md). |
| Auto-hide delay                          | `autoHide.delayMs`                | `250`        | How long Stem waits before it hides the dock, in milliseconds, 0 to 5000. The delay counts from the moment the pointer leaves the dock, or a window starts to cover it. |

## Items

| Setting      | Key                 | Default | What it does |
| ------------ | ------------------- | ------- | ------------ |
| Start        | `items.launcher`    | on      | The Start item, at the left. |
| Task View    | `items.taskView`    | off     | The Task View item, at the left. |
| Show Desktop | `items.showDesktop` | off     | The Show Desktop item, at the left. |
| Recycle Bin  | `items.recycleBin`  | on      | The Recycle Bin item, at the right. |
| Separators   | `items.separators`  | on      | Draws thin lines between the fixed items, the pinned apps and the other running apps. |

The first four are the fixed items. [Fixed items](fixed-items.md) describes
what each one does.

## Pinned apps

The _Pinned apps_ section of the window lists the pinned apps.
[The Pinned apps section](pinned-apps.md#the-pinned-apps-section) describes
what it shows and the `pinned` array behind it.

## Position & monitors

| Setting  | Key        | Default   | What it does |
| -------- | ---------- | --------- | ------------ |
| Position | `position` | `bottom`  | The edge of the screen the dock is on. Only _Bottom_ (`bottom`) works. _Left_ (`left`), _Right_ (`right`) and _Top_ (`top`) are greyed out in the window, and with one of them in the file the dock stays at the bottom. |
| Monitors | `monitors` | `primary` | _Primary monitor only_ (`primary`) gives one dock, on the primary monitor. _All monitors_ (`all`) gives one dock on every monitor. See [More than one monitor](multiple-monitors.md). |

## System

| Setting                     | Key                         | Default | What it does |
| --------------------------- | --------------------------- | ------- | ------------ |
| Start Stem when you sign in | `system.autostart`          | off     | Adds Stem to the apps Windows starts when you sign in. See [Starting with Windows](starting-with-windows.md). |
| Hide the Windows taskbar    | `system.hideWindowsTaskbar` | off     | Turns on Windows' own taskbar auto-hide while Stem runs, and puts your setting back when Stem quits. See [Hiding the Windows taskbar](hiding-the-windows-taskbar.md). |
| Reserve screen space        | `system.reserveScreenSpace` | off     | Greyed out. It is not implemented, so maximised windows still extend under the dock. |
| Write a log file            | `system.fileLogging`        | on      | Writes a log file in the app data folder. Stem reads this setting when it starts, so a change applies after a restart (the tray icon's _Restart_ does one). See [The log file](crashes-and-logs.md#the-log-file). |

The file also has a `version` field, which is `1`. Stem sets it, so leave it
as it is.

## About

The last section of the window is not a setting. It shows Stem's version,
the path of the settings file and the path of the log file. Each path has an
_Open folder_ button, which opens its folder in File Explorer. While file
logging is off, the log row says _File logging is off_ and its button is
disabled.
