# Widget items

This file describes the clock, battery and volume items, and the clock's regional format.

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
  the first one every field is `false` and `percent` is `null`. A new reading sends a
  `dock://state` only while the battery item is shown, or when it adds or removes that item
  (v1.31; see [Readings no item shows](state-snapshot.md#readings-no-item-shows-v131)). The
  battery item exists only while `items.battery` is on **and** `power.hasBattery` is `true`, so a
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
  `hour: 'numeric', minute: '2-digit'`, in the page's locale or in the regional format from Rust,
  see [The clock's regional format](#the-clocks-regional-format-v125) below) over the short
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
  `setPower(status)`, which emits a new state as Rust does (v1.31). The clock follows the page's
  own clock (Playwright installs a fixed one).

### The clock's regional format (v1.25)

The page's own locale is WebView2's language, which follows the Windows display language, not the
regional format. With an English display language and a 24-hour region, the page would show
`10:42 AM` where the taskbar shows `10:42`. So the format comes from Rust:

- `DockState.clockFormat: ClockFormat` is `{ locale: string | null, hour12: boolean | null }`:
  the user's regional format as Windows stores it.
  - `locale` is `GetUserDefaultLocaleName` (a BCP 47 name such as `en-GB`), with an alternate
    sort order removed (`de-DE_phoneb` becomes `de-DE`), because Intl rejects the underscore.
  - `hour12` is `false` when the user's short time format
    (`GetLocaleInfoEx(LOCALE_NAME_USER_DEFAULT, LOCALE_SSHORTTIME)`) uses `H` (24-hour) and
    `true` when it uses `h`. It is read from the first `H` or `h` outside single quotes (quoted
    text is literal). `LOCALE_SSHORTTIME` may be a list delimited by semicolons, so the first
    hour letter is the one of its first entry, the preferred one.
  - Either field is `null` when Windows does not report it, and `hour12` is also `null` when the
    pattern has no hour letter.

  The engine stores the last `PlatformEvent::ClockFormatChanged(ClockFormat)`; until the first
  one both fields are `null`.
- The platform side is `Platform::clock_format()`. The events thread reads it after the start
  snapshot (after the power reading). On a `WM_SETTINGCHANGE` whose `lParam` is the string
  `intl`, it reads it at once and again 500 ms later. It sends `ClockFormatChanged` only when the
  reading differs from the last one sent.
- The page builds its `Intl.DateTimeFormat`s for the clock face, its title pill and its
  accessible name with `locale` (when it is not `null`). When `hour12` is not `null`, it adds
  `hourCycle` `h12` for `true` and `h23` for `false`: Windows' `h` is 1 to 12 and `H` is 0 to
  23, and Intl's `hour12: true` is 0 to 11 in some locales such as ja-JP. The page falls back to
  its own locale when Intl rejects the name. A custom Windows pattern (literal text, a seconds
  field, the hour's digits, Windows' AM/PM symbols) is not copied; only the hour cycle and the
  locale's order are used.
- `MockBackend`: `MockOptions.clockFormat` (default: both fields `null`) and
  `setClockFormat(format)`, which emits a new state. The mock platform sends `ClockFormatChanged`
  once at start, after its `PowerChanged`; its demo reading also has both fields `null`.

## The volume item (v1.30)

Another widget: the default output device's volume as an optional fixed item.

- `Settings.items.volume`, `false` by default. `DockItemKind` has `volume`, with item key
  `fixed:volume` and icon key `shell:volume` (drawn by the page, like the other `shell:` keys).
  It is in the trailing group after `notificationCenter` and before the battery, in the order of
  the Windows taskbar's corner: volume, battery, clock. It is shown whenever the setting is on,
  with or without an output device.
- `DockState.volume: VolumeStatus | null` is `{ percent: number, muted: boolean }`: the default
  output device's master volume and whether it is muted. `percent` is from 0 to 100 (the
  endpoint's level scalar, 0.0 to 1.0, × 100, rounded half away from zero). The level is kept
  while muted. The value is `null` when there is no default output device or its volume cannot be
  read. The engine stores the last `PlatformEvent::VolumeChanged(Option<VolumeStatus>)` whatever
  the settings say; it is `null` until the first one. A new reading sends a `dock://state` only
  while the item is shown (v1.31; see
  [Readings no item shows](state-snapshot.md#readings-no-item-shows-v131)), so a slider drag with
  the item off sends nothing. Rust sets the item's `title`: `Volume 45 %`, `Volume muted` while
  muted (whatever the level), and `No audio output` without a device.
- The platform side:
  - `Platform::volume_status()` returns a reading now.
  - `set_volume(percent)` sets the level (more than 100 counts as 100) and leaves the mute alone.
  - `set_volume_muted(muted)` sets the mute and leaves the level alone. Both setters fail without
    an output device.
  - `step_volume(direction)` handles one wheel notch. It takes the reading now, applies
    `stem_core::engine::volume_step` to it, and sets the level, then the mute, where they
    change, as one operation that no other volume request interleaves with. It returns the
    volume it set, or `null` when there was nothing to do.

  On Windows a dedicated thread of the platform layer (`stem-volume`, in the multithreaded
  apartment) follows the default console render endpoint
  (`IMMDeviceEnumerator::GetDefaultAudioEndpoint(eRender, eConsole)` → `IAudioEndpointVolume`:
  `GetMasterVolumeLevelScalar`, `GetMute`, `SetMasterVolumeLevelScalar`, `SetMute`). An
  `IAudioEndpointVolumeCallback` on the endpoint and an `IMMNotificationClient` on the
  enumerator only post to the thread. The endpoint callback posts for every level or mute change.
  The notification client posts for a new default of `eRender`/`eConsole` and for any device's
  state change, arrival or removal; the thread then binds again when the default's id differs.

  The thread reports a reading only when it differs from the last one reported, and at most one
  per 50 ms. A change after 50 ms without changes goes out at once. The rest of a burst (a
  slider drag) goes out once at the end of the interval, with the value read then, so the last
  value always arrives. A failure reads `null` and is logged at `warn` once, then at `debug`
  while the failures continue. It is always tried again on a timer, never left to a
  notification alone: with no device every 60 s (a device notification ends the wait sooner),
  for anything else after 2 s, doubling up to 60 s (v1.31). Every timed retry builds the
  enumerator and its notification client again, because a restart of the audio service may leave
  the old registration without callbacks. A failure other than "no device" drops them at once.
  While a device is bound and reads, the thread has no timer.

  Each method is one request to the thread and waits at most 250 ms for it
  (`PlatformError::Timeout`). A set or a step that the thread takes only after its caller's wait
  has ended is dropped, not done late. The first reading is reported once the thread has bound
  the device at start.
- The page draws the face: a speaker with no wave at 0 %, one wave from 1 % to 33 %, two from
  34 % to 66 % and three from 67 % (Windows' three steps), with the level below as text in the
  page's locale. While `muted`, a cross replaces the waves and the level is still written. For
  `null` it draws a dimmed, struck-out speaker and a dash. The title pill and accessible name are
  the `title`.
- Click (and Enter or Space): `ShellAction` `openQuickSettings` (Win+A), which has Windows'
  volume slider, as Windows' own volume icon does.
- The wheel: the page sends `scroll_item` for every notch over the volume item, whatever
  `behavior.scrollAction` says. It counts notches as in
  [Scrolling over an item](scrolling-over-an-item.md); over every other item it still sends only
  while the setting is not `none`. Rust (`ScrollEffect::StepVolume`) calls `Platform::step_volume`
  once per notch, off the async runtime's workers (on Tauri's blocking pool). So each step starts
  from the volume now, not from the snapshot, which falls behind during a quick run of notches.
  `previous` (the wheel turned away from the user, up) raises the volume by 2, up to 100, and
  unmutes, as the volume-up key does. `next` lowers it by 2, down to 0, and keeps the mute. The
  platform runs the steps one after the other, so notches that arrive together add up.

  The 150 ms limit of [Scrolling over an item](scrolling-over-an-item.md) does **not** apply:
  each notch the page sends counts (one page event sends at most ten), as each press of a volume
  key does. Without an output device, or at the end of the range, a notch does nothing and
  resolves. So does a notch that the platform did not complete in time (a `warn` line, then
  `debug` lines until a step succeeds): it is dropped rather than reported to the page, and the
  failure affects only that notch. With the item off, its key is unknown and the command rejects,
  as for a click. No window comes to the front, so the focus shortcut's stored window stays.
- Menu (built by Rust): *Mute*, or *Unmute* while the engine's last reading says muted
  (`set_volume_muted`; disabled without an output device; it does not supersede the focus
  shortcut's stored window), *Sound settings* (`ShellAction` `openSoundSettings`, which launches
  `ms-settings:sound`) and *Volume mixer* (`openVolumeMixer`: `ms-settings:apps-volume`, which
  Microsoft documents as *App volume and device preferences*, and which is *Sound > Volume mixer*
  on Windows 11). The entries that every item menu ends with follow.
- `MockBackend`: `MockOptions.volume` (default: a device at 60 %, not muted; `null` for none),
  `setVolume(status | null)` (a change made outside Stem, which emits a new state when it differs
  and the item is shown, v1.31) and `chooseVolumeMute(muted)` (what the menu's *Mute* or *Unmute*
  runs). It records `stepVolume` (one per notch over the item, as the service calls
  `Platform::step_volume`) and `setVolumeMuted`, and it steps without the 150 ms limit, like Rust.
  The mock platform's demo has the same device and sends its `VolumeChanged` once at start, after
  `ClockFormatChanged`.
