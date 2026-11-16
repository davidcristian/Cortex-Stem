<script lang="ts">
  import type { DockPosition } from '$lib/ipc/generated/DockPosition';
  import type { Settings } from '$lib/ipc/generated/Settings';

  import { millis } from './format';
  import Select from './Select.svelte';
  import SettingRow from './SettingRow.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import Slider from './Slider.svelte';
  import TextField from './TextField.svelte';
  import Toggle from './Toggle.svelte';

  interface Props {
    settings: Settings;
    /** `SettingsStore.edit`: changes a copy of the document and saves it. */
    edit: (mutate: (draft: Settings) => void) => void;
  }

  let { settings, edit }: Props = $props();

  /**
   * Where a hidden dock comes back from: the screen's edge on the
   * dock's own side, the reveal strip of ADR-0008, in the auto-hide row.
   */
  const REVEAL_EDGE: Record<DockPosition, string> = {
    bottom: "the screen's bottom edge below it",
    top: "the screen's top edge above it",
    left: "the screen's left edge beside it",
    right: "the screen's right edge beside it",
  };
</script>

<SettingsCard>
  <SettingRow
    id="click-active-minimizes"
    label="Clicking the active app minimises it"
    description="Otherwise the click brings its window to the front again"
  >
    <Toggle
      id="click-active-minimizes"
      checked={settings.behavior.clickActiveMinimizes}
      onchange={(v) =>
        edit((s) => {
          s.behavior.clickActiveMinimizes = v;
        })}
    />
  </SettingRow>
  <!-- The wheel over an item (docs/ipc/scrolling-over-an-item.md). -->
  <SettingRow
    id="scroll-action"
    label="Scrolling over an app"
    description="What the mouse wheel does over an app's icon. With Cycles its windows, each notch of the wheel over an app brings its next open window to the front, or the previous one when you scroll up; a minimised window is restored."
  >
    <Select
      id="scroll-action"
      value={settings.behavior.scrollAction}
      options={[
        { value: 'none', label: 'Does nothing' },
        { value: 'cycleWindows', label: 'Cycles its windows' },
      ]}
      onchange={(v) =>
        edit((s) => {
          s.behavior.scrollAction = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="current-desktop"
    label="Show only windows on the current desktop"
    description="Hide windows that live on another virtual desktop"
  >
    <Toggle
      id="current-desktop"
      checked={settings.behavior.showOnlyCurrentDesktop}
      onchange={(v) =>
        edit((s) => {
          s.behavior.showOnlyCurrentDesktop = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="current-monitor"
    label="Show only windows on the dock's monitor"
    description="Applies with one dock per monitor (Monitors: All monitors): each dock lists the windows on its monitor; pinned apps stay on all"
    disabled={settings.monitors !== 'all'}
  >
    <Toggle
      id="current-monitor"
      checked={settings.behavior.showOnlyCurrentMonitor}
      disabled={settings.monitors !== 'all'}
      onchange={(v) =>
        edit((s) => {
          s.behavior.showOnlyCurrentMonitor = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="running-indicators"
    label="Running indicators"
    description="Dots under apps that have open windows"
  >
    <Toggle
      id="running-indicators"
      checked={settings.behavior.runningIndicators}
      onchange={(v) =>
        edit((s) => {
          s.behavior.runningIndicators = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="click-through"
    label="Click through outside the dock"
    description="Clicks beside and above the panel reach the desktop and the windows behind it"
  >
    <Toggle
      id="click-through"
      checked={settings.behavior.clickThrough}
      onchange={(v) =>
        edit((s) => {
          s.behavior.clickThrough = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="toggle-shortcut"
    label="Toggle the dock"
    description="Shows or hides the dock from any app. Ctrl, Alt, Shift or Super, then one key, joined by +, like Ctrl+Alt+D; empty for none."
  >
    <TextField
      id="toggle-shortcut"
      value={settings.behavior.toggleShortcut}
      placeholder="None"
      describedby="toggle-shortcut-description"
      oncommit={(v) =>
        edit((s) => {
          s.behavior.toggleShortcut = v;
        })}
    />
  </SettingRow>
  <!-- Keyboard entry into the dock (docs/ipc/keyboard-entry.md). -->
  <SettingRow
    id="focus-shortcut"
    label="Focus the dock"
    description="Moves the keyboard into the dock from any app: the arrows go from icon to icon, Enter opens one, Esc goes back. Same format as the toggle, like Ctrl+Alt+Shift+D, and not the same keys; empty for none."
  >
    <TextField
      id="focus-shortcut"
      value={settings.behavior.focusShortcut}
      placeholder="None"
      describedby="focus-shortcut-description"
      oncommit={(v) =>
        edit((s) => {
          s.behavior.focusShortcut = v;
        })}
    />
  </SettingRow>
  <!-- ADR-0008: the hidden dock leaves its window
       behind, which takes clicks unless click-through is on. -->
  <SettingRow
    id="auto-hide"
    label="Auto-hide"
    description="Always: slides the dock away whenever the pointer leaves it. When a window overlaps: only while a window covers the dock, and back as soon as none does. Touch {REVEAL_EDGE[
      settings.position
    ]} to bring it back. Works best with click-through on: with it off, the space the hidden dock leaves still takes clicks."
  >
    <Select
      id="auto-hide"
      value={settings.autoHide.mode}
      options={[
        { value: 'off', label: 'Off' },
        { value: 'always', label: 'Always' },
        { value: 'whenOverlapped', label: 'When a window overlaps' },
      ]}
      onchange={(v) =>
        edit((s) => {
          s.autoHide.mode = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="auto-hide-delay"
    label="Auto-hide delay"
    description="Wait this long after the pointer leaves, or after a window starts to overlap the dock, before hiding"
  >
    <Slider
      id="auto-hide-delay"
      min={0}
      max={5000}
      step={50}
      value={settings.autoHide.delayMs}
      format={millis}
      onchange={(v) =>
        edit((s) => {
          s.autoHide.delayMs = v;
        })}
    />
  </SettingRow>
</SettingsCard>
