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
  <SettingRow
    id="autostart"
    label="Start Stem when you sign in"
    description="Adds Stem to the apps Windows starts for you"
  >
    <Toggle
      id="autostart"
      checked={settings.system.autostart}
      onchange={(v) =>
        edit((s) => {
          s.system.autostart = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="hide-taskbar"
    label="Hide the Windows taskbar"
    description="Windows hides it automatically while Stem runs; put back when Stem quits"
  >
    <Toggle
      id="hide-taskbar"
      checked={settings.system.hideWindowsTaskbar}
      onchange={(v) =>
        edit((s) => {
          s.system.hideWindowsTaskbar = v;
        })}
    />
  </SettingRow>
  <!-- ADR-0010: Rust registers each dock window as an
       AppBar; a dock that auto-hides reserves nothing, so the
       switch is greyed out while Auto-hide is on. -->
  <SettingRow
    id="reserve-space"
    label="Reserve screen space"
    description="Maximised windows stop short of the dock instead of going under it. Applies while Auto-hide is off: a dock that hides itself reserves nothing."
    disabled={settings.autoHide.mode !== 'off'}
  >
    <Toggle
      id="reserve-space"
      checked={settings.system.reserveScreenSpace}
      disabled={settings.autoHide.mode !== 'off'}
      onchange={(v) =>
        edit((s) => {
          s.system.reserveScreenSpace = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="file-logging"
    label="Write a log file"
    description="In the app data folder. Applies on restart."
  >
    <Toggle
      id="file-logging"
      checked={settings.system.fileLogging}
      onchange={(v) =>
        edit((s) => {
          s.system.fileLogging = v;
        })}
    />
  </SettingRow>
</SettingsCard>
