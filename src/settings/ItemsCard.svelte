<script lang="ts">
  import type { Settings } from '$lib/ipc/generated/Settings';

  import SettingRow from './SettingRow.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import Toggle from './Toggle.svelte';

  interface Props {
    settings: Settings;
    /** `SettingsStore.edit`: changes a copy of the document and saves it. */
    edit: (mutate: (draft: Settings) => void) => void;
  }

  let { settings, edit }: Props = $props();
</script>

<SettingsCard>
  <SettingRow id="item-launcher" label="Start" description="Opens the Start menu">
    <Toggle
      id="item-launcher"
      checked={settings.items.launcher}
      onchange={(v) =>
        edit((s) => {
          s.items.launcher = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="item-task-view"
    label="Task View"
    description="Shows your open windows and desktops"
  >
    <Toggle
      id="item-task-view"
      checked={settings.items.taskView}
      onchange={(v) =>
        edit((s) => {
          s.items.taskView = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="item-show-desktop"
    label="Show Desktop"
    description="Minimises every window; a second click brings them back"
  >
    <Toggle
      id="item-show-desktop"
      checked={settings.items.showDesktop}
      onchange={(v) =>
        edit((s) => {
          s.items.showDesktop = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="item-quick-settings"
    label="Quick Settings"
    description="Opens Windows' quick settings: network, volume, battery"
  >
    <Toggle
      id="item-quick-settings"
      checked={settings.items.quickSettings}
      onchange={(v) =>
        edit((s) => {
          s.items.quickSettings = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="item-notification-center"
    label="Notification Center"
    description="Opens your notifications and the calendar"
  >
    <Toggle
      id="item-notification-center"
      checked={settings.items.notificationCenter}
      onchange={(v) =>
        edit((s) => {
          s.items.notificationCenter = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="item-volume"
    label="Volume"
    description="The speaker's volume; scroll over it to change it. Opens quick settings"
  >
    <Toggle
      id="item-volume"
      checked={settings.items.volume}
      onchange={(v) =>
        edit((s) => {
          s.items.volume = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="item-battery"
    label="Battery"
    description="The battery's charge; only on a PC with a battery. Opens quick settings"
  >
    <Toggle
      id="item-battery"
      checked={settings.items.battery}
      onchange={(v) =>
        edit((s) => {
          s.items.battery = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="item-clock"
    label="Clock"
    description="The time and the date. Opens your notifications and the calendar"
  >
    <Toggle
      id="item-clock"
      checked={settings.items.clock}
      onchange={(v) =>
        edit((s) => {
          s.items.clock = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="item-recycle-bin"
    label="Recycle Bin"
    description="Shows whether the bin is empty and opens it; files dropped on it go to the bin"
  >
    <Toggle
      id="item-recycle-bin"
      checked={settings.items.recycleBin}
      onchange={(v) =>
        edit((s) => {
          s.items.recycleBin = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="item-separators"
    label="Separators"
    description="Thin lines between the fixed, pinned and running groups"
  >
    <Toggle
      id="item-separators"
      checked={settings.items.separators}
      onchange={(v) =>
        edit((s) => {
          s.items.separators = v;
        })}
    />
  </SettingRow>
</SettingsCard>
