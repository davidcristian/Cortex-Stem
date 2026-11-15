# Widget items

This file describes the clock and battery items.

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
  The thread sends `PowerChanged` once after the start snapshot, and after that only when the
  status differs from the last one sent. It unregisters the notifications at stop.
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
  `ms-settings:powersleep`). Both end with the entries that every item menu ends with
  ([Context menus](context-menus.md)).
- `MockBackend`: `MockOptions.power` (default: a laptop at 76 %, on battery) and
  `setPower(status)`, which emits a new state. The clock follows the page's
  own clock (Playwright installs a fixed one).
