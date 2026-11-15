# Widget items

This file describes the clock and battery items, and the clock's regional format.

## Clock and battery items (v1.22)

Two optional fixed items that are widgets, items that draw live data:

- `Settings.items.clock` and `Settings.items.battery`, both `false` by default. `DockItemKind`
  has `clock` and `battery`, with item keys `fixed:clock` and `fixed:battery` and icon keys
  `shell:clock` and `shell:battery` (drawn by the page, like the other `shell:` keys). They are in
  the trailing group after `notificationCenter` and before the Recycle Bin, in the order battery,
  clock.
- `DockState.power: PowerStatus` is `{ hasBattery: boolean, percent: number | null, charging:
  boolean, onAc: boolean, saver: boolean }` (`percent` from 0 to 100, `null` when the system does
  not report it). The engine stores the last `PlatformEvent::PowerChanged(PowerStatus)`; until
  the first one every field is `false` and `percent` is `null`. The battery item exists only
  while `items.battery` is on **and** `power.hasBattery` is `true`, so a
  desktop PC never shows it. Rust sets its `title`: `Battery 73 %`, `Battery 73 %, charging`,
  `Battery 73 %, plugged in` (on AC, not charging), with `, battery saver on` appended while
  `saver` is set, and `Battery` when `percent` is `null`. The clock item's `title` is `Clock`.
- The platform side is `Platform::power_status()`. On Windows it calls `GetSystemPowerStatus` (no
  system battery: `BatteryFlag` 128; unknown percent: 255; charging: flag 8; on AC:
  `ACLineStatus` 1; battery saver: `SystemStatusFlag` 1). The events thread registers its window
  for `GUID_BATTERY_PERCENTAGE_REMAINING`, `GUID_ACDC_POWER_SOURCE` and
  `GUID_POWER_SAVING_STATUS` (`RegisterPowerSettingNotification`), and also handles
  `PBT_APMPOWERSTATUSCHANGE` and `PBT_APMRESUMEAUTOMATIC`. Each of these reads the status again.
  Once a `GUID_POWER_SAVING_STATUS` notification has arrived, battery saver comes from the
  `DWORD` of the last one (the window procedure copies it out of the `POWERBROADCAST_SETTING`)
  instead of from `SystemStatusFlag`. The thread sends `PowerChanged` once after the start
  snapshot, and after that only when the status differs from the last one sent. It unregisters
  the notifications at stop.
- The page draws the faces. The clock shows the time (`Intl.DateTimeFormat` with
  `hour: 'numeric', minute: '2-digit'`, in the page's locale) over the short
  weekday and the day. Its title pill and accessible name are the full date and time. It updates
  at each minute boundary with one
  `setTimeout` (none while no clock item is shown), and again on `visibilitychange`. The battery
  shows a battery glyph filled to `percent`, a bolt while `charging`, a plug while `onAc` and not
  charging, the percent as text, and a distinct fill while `saver` is set.
- Clicks: the clock runs `ShellAction` `openNotificationCenter` (Win+N, which on Windows 11
  shows the calendar with the notifications); the battery runs `openQuickSettings` (Win+A).
  Menus (built by Rust): the clock's has *Open Notification Center* and *Date and time settings*
  (`ShellAction` `openDateTimeSettings`, which launches `ms-settings:dateandtime`). The
  battery's has *Open Quick Settings* and *Power and battery settings* (`openPowerSettings`:
  `ms-settings:powersleep`, which Microsoft documents as System > Power & sleep; on Windows 11 the
  page is Power & battery, or Power on a PC without a battery). Both end with the entries that
  every item menu ends with ([Context menus](context-menus.md)).
- `MockBackend`: `MockOptions.power` (default: a laptop at 76 %, on battery) and
  `setPower(status)`, which emits a new state. The clock follows the page's
  own clock (Playwright installs a fixed one).

### The clock's regional format (v1.25)

The page's own locale is WebView2's language, which follows the Windows display language, not the
regional format. With an English display language and a 24-hour region, the page would show
`10:42 AM` where the taskbar shows `10:42`. So the format comes from Rust:

- `DockState.clockFormat: ClockFormat` is `{ locale: string | null, hour12: boolean | null }`:
  the user's regional format as Windows stores it.
  - `locale` is `GetUserDefaultLocaleName` (a BCP 47 name such as `en-GB`).
  - `hour12` is `false` when the user's short time format
    (`GetLocaleInfoEx(LOCALE_NAME_USER_DEFAULT, LOCALE_SSHORTTIME)`) uses `H` (24-hour) and
    `true` when it uses `h`.
  - Either field is `null` when Windows does not report it.

  The engine stores the last `PlatformEvent::ClockFormatChanged(ClockFormat)`; until the first
  one both fields are `null`.
- The platform side is `Platform::clock_format()`. The events thread reads it after the start
  snapshot, and again on a `WM_SETTINGCHANGE` whose `lParam` is the string `intl`. It sends
  `ClockFormatChanged` only when the reading differs from the last one sent.
- The page builds its `Intl.DateTimeFormat`s for the clock face, its title pill and its
  accessible name with `locale` (when it is not `null`) and `hour12` (when it is not `null`). The
  page falls back to its own locale when Intl rejects the name. A custom Windows pattern (literal
  text, a seconds field) is not copied; only the hour cycle and the locale's order are used.
- `MockBackend`: `MockOptions.clockFormat` (default: both fields `null`) and
  `setClockFormat(format)`, which emits a new state.
