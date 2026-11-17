<script lang="ts">
  // Built-in vector art for the `shell:` icon namespace (docs/ipc/icon-keys.md).
  // Tiles use the same 22 % corner radius as app icons so the row reads as one.
  import type { PowerStatus } from '$lib/ipc/generated/PowerStatus';
  import type { VolumeStatus } from '$lib/ipc/generated/VolumeStatus';

  import { batteryFace, DAY_PERIOD_SCALE, timeFontSize, volumeFace, type ClockText } from './faces';
  import FixedShellIcon from './FixedShellIcon.svelte';

  interface Props {
    /** The key without its `shell:` prefix, e.g. `start`, `recycle-bin-full`. */
    name: string;
    /**
     * The clock face's text (`shell:clock`), already in the user's
     * regional format (`clockText`); `null` draws the bare tile.
     */
    clock?: ClockText | null;
    /** `DockState.power`, for the battery face (`shell:battery`). */
    power?: PowerStatus | null;
    /**
     * `DockState.volume`, for the volume face (`shell:volume`):
     * `null` without an output device.
     */
    volume?: VolumeStatus | null;
  }

  let { name, clock = null, power = null, volume = null }: Props = $props();

  const battery = $derived(name === 'battery' ? batteryFace(power) : null);
  const speaker = $derived(name === 'volume' ? volumeFace(volume) : null);
  const timeSize = $derived(clock === null ? 0 : timeFontSize(clock));
  /** The day period with the space that parts it from the time, before or after it. */
  const periodBefore = $derived(
    clock !== null && clock.periodFirst && clock.dayPeriod !== '' ? `${clock.dayPeriod} ` : '',
  );
  const periodAfter = $derived(
    clock !== null && !clock.periodFirst && clock.dayPeriod !== '' ? ` ${clock.dayPeriod}` : '',
  );
</script>

<svg viewBox="0 0 64 64" aria-hidden="true" focusable="false">
  {#if name === 'clock'}
    <!-- The time over the short weekday and day, in the user's
         regional format (DockState.clockFormat); the day period of a
         12-hour clock at half size, a 24-hour time larger without one. -->
    <rect x="2" y="2" width="60" height="60" rx="14" fill="var(--tile-neutral)" />
    {#if clock !== null}
      <text class="face time" x="32" y="31" font-size={timeSize} text-anchor="middle"
        ><tspan class="period" font-size={timeSize * DAY_PERIOD_SCALE}>{periodBefore}</tspan><tspan
          >{clock.time}</tspan
        ><tspan class="period" font-size={timeSize * DAY_PERIOD_SCALE}>{periodAfter}</tspan></text
      >
      <text class="face date" x="32" y="49" font-size="12" text-anchor="middle"
        >{clock.weekday} {clock.day}</text
      >
    {/if}
  {:else if name === 'battery' && battery !== null}
    <!-- A battery filled to the charge, a bolt while charging, a plug on AC
         power without charging, the percent below; amber while battery
         saver is on, red below 20 % while not charging, and never
         a fill too thin to see (`batteryFace`). The glyph is
         outlined in the panel's tint so it reads over the fill as well as
         over the empty part. -->
    <rect x="2" y="2" width="60" height="60" rx="14" fill="var(--tile-neutral)" />
    <rect
      x="10"
      y="15"
      width="40"
      height="22"
      rx="5"
      fill="none"
      stroke="currentColor"
      stroke-width="3"
    />
    <rect x="51.5" y="21.5" width="3.5" height="9" rx="1.5" fill="currentColor" />
    {#if battery.fill !== null}
      <rect
        class="fill"
        class:saver={battery.saver}
        class:low={battery.low}
        x="14"
        y="19"
        width={battery.fill}
        height="14"
        rx="2"
      />
    {/if}
    {#if battery.glyph === 'bolt'}
      <path class="glyph" d="M33 16.5 L24.5 27.5 H30.5 L28 35.5 L36.5 24.5 H30.5 Z" />
    {:else if battery.glyph === 'plug'}
      <path
        class="glyph"
        d="M26.5 17h2v4h3v-4h2v4h2v4.5a5.5 5.5 0 0 1-4.5 5.4V35h-2v-4.1a5.5 5.5 0 0 1-4.5-5.4V21h2Z"
      />
    {/if}
    <text class="face percent" x="32" y="53" font-size="14" text-anchor="middle"
      >{battery.percentText}</text
    >
  {:else if name === 'volume' && speaker !== null}
    <!-- A speaker with one to three waves by level (none at 0 %), a cross
         where the waves would be while muted, struck out and dimmed
         without an output device; the level below as text
         (`volumeFace`). The slash is outlined in the panel's tint so it
         reads over the speaker, as the battery's glyph does. -->
    <rect x="2" y="2" width="60" height="60" rx="14" fill="var(--tile-neutral)" />
    <g class="speaker" class:none={speaker.state === 'none'}>
      <path class="cone" d="M17 22.5h6l8.5-7.5v23l-8.5-7.5h-6Z" />
      {#if speaker.waves >= 1}
        <path class="wave" d="M34.95 21.55A7 7 0 0 1 34.95 31.45" />
      {/if}
      {#if speaker.waves >= 2}
        <path class="wave" d="M38.49 18.01A12 12 0 0 1 38.49 34.99" />
      {/if}
      {#if speaker.waves >= 3}
        <path class="wave" d="M42.02 14.48A17 17 0 0 1 42.02 38.52" />
      {/if}
      {#if speaker.state === 'muted'}
        <path class="cross" d="M36.5 22l9 9M45.5 22l-9 9" />
      {/if}
    </g>
    {#if speaker.state === 'none'}
      <path class="slash" d="M15 13L49 40" />
    {/if}
    <text
      class="face percent"
      class:quiet={speaker.state !== 'on'}
      x="32"
      y="53"
      font-size="14"
      text-anchor="middle">{speaker.percentText}</text
    >
  {:else}
    <FixedShellIcon {name} />
  {/if}
</svg>

<style>
  svg {
    display: block;
    width: 100%;
    height: 100%;
    color: var(--text);
  }

  .face {
    fill: currentColor;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  /* The text colour, a little quieter than the time; the accent was too
     faint on the dark tile. */
  .date {
    opacity: 0.75;
  }

  .fill {
    fill: currentColor;
  }

  /* The battery's warning colours, theme tokens like those
     of app.css: battery saver's amber, as Windows shows it, on either
     panel tint; a low charge in Windows 11's critical colour (Fluent's
     SystemFillColorCritical), lighter on the dark panel. Set on the root
     with no specificity (`:where`), the light value after the dark one,
     so any rule that sets them wins in both colour modes, a plain `:root`
     in a dock.css included. They are internal, not among the stable
     variables of docs/THEMING.md, and may change with an update. */
  :global(:where(:root)) {
    --battery-saver: #f2a900;
    --battery-low: #ff99a4;
  }

  :global(:where(:root[data-theme='light'])) {
    --battery-low: #c42b1c;
  }

  .fill.saver {
    fill: var(--battery-saver);
  }

  .fill.low {
    fill: var(--battery-low);
  }

  .glyph {
    fill: currentColor;
    stroke: rgb(var(--panel-tint));
    stroke-width: 1.5;
    stroke-linejoin: round;
    paint-order: stroke;
  }

  /* The volume face: the text colour, like the other tiles; the
     speaker and its text quieter without a device, the text while muted. */
  .cone {
    fill: currentColor;
    stroke: currentColor;
    stroke-width: 1;
    stroke-linejoin: round;
  }

  .wave,
  .cross {
    fill: none;
    stroke: currentColor;
    stroke-width: 3;
    stroke-linecap: round;
  }

  .speaker.none {
    opacity: 0.45;
  }

  .slash {
    fill: none;
    stroke: currentColor;
    stroke-width: 3;
    stroke-linecap: round;
    /* An outline in the panel's tint parts it from the speaker under it. */
    filter: drop-shadow(0 0 1.5px rgb(var(--panel-tint)));
  }

  .percent.quiet {
    opacity: 0.6;
  }
</style>
