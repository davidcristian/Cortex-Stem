/**
 * The power, volume and regional-format readings of the mock platform, and
 * the titles and the wheel step the engine makes of them (engine/snapshot.rs,
 * engine/scroll.rs).
 */

import type { ClockFormat } from '$lib/ipc/generated/ClockFormat';
import type { PowerStatus } from '$lib/ipc/generated/PowerStatus';
import type { ScrollDirection } from '$lib/ipc/generated/ScrollDirection';
import type { VolumeStatus } from '$lib/ipc/generated/VolumeStatus';

/**
 * The demo's power reading: a laptop at 76 %, on battery; the same
 * as `demo_power` in `crates/stem-platform/src/mock/demo.rs`.
 */
export const DEMO_POWER: Readonly<PowerStatus> = Object.freeze({
  hasBattery: true,
  percent: 76,
  charging: false,
  onAc: false,
  saver: false,
});

/** What the engine knows before the first power reading: nothing. */
export const NO_POWER: Readonly<PowerStatus> = Object.freeze({
  hasBattery: false,
  percent: null,
  charging: false,
  onAc: false,
  saver: false,
});

/**
 * The demo's output device: at 60 %, not muted; the same as
 * `demo_volume` in `crates/stem-platform/src/mock/demo.rs`.
 */
export const DEMO_VOLUME: Readonly<VolumeStatus> = Object.freeze({ percent: 60, muted: false });

/**
 * How far one wheel notch over the volume item moves the volume, in
 * percentage points (`VOLUME_STEP` in engine/scroll.rs).
 */
export const VOLUME_STEP = 2;

/**
 * What one wheel notch over the volume item makes of `current`
 * (`volume_step` in engine/scroll.rs): `previous` (the wheel turned away
 * from the user, up) raises it by `VOLUME_STEP` up to 100 and unmutes;
 * `next` lowers it by `VOLUME_STEP` down to 0 and keeps the mute.
 */
export function volumeStep(current: VolumeStatus, direction: ScrollDirection): VolumeStatus {
  const percent = Math.min(100, current.percent);
  return direction === 'previous'
    ? { percent: Math.min(100, percent + VOLUME_STEP), muted: false }
    : { percent: Math.max(0, percent - VOLUME_STEP), muted: current.muted };
}

/**
 * The volume item's title (`volume_title` in engine/snapshot.rs):
 * `Volume 45 %`, `Volume muted` while muted, `No audio output` without a
 * default output device.
 */
export function volumeTitle(volume: VolumeStatus | null): string {
  if (volume === null) {
    return 'No audio output';
  }
  return volume.muted ? 'Volume muted' : `Volume ${volume.percent} %`;
}

/** What the engine knows before the first regional-format reading: nothing. */
export const NO_CLOCK_FORMAT: Readonly<ClockFormat> = Object.freeze({ locale: null, hour12: null });

/**
 * The battery item's title (`battery_title` in engine/snapshot.rs):
 * `Battery 73 %`, then `, charging` or `, plugged in` (on AC, not charging),
 * then `, battery saver on`; `Battery` alone, whatever else is known, without
 * a known percent.
 */
export function batteryTitle(power: PowerStatus): string {
  if (power.percent === null) {
    return 'Battery';
  }
  let title = `Battery ${power.percent} %`;
  if (power.charging) {
    title += ', charging';
  } else if (power.onAc) {
    title += ', plugged in';
  }
  if (power.saver) {
    title += ', battery saver on';
  }
  return title;
}
