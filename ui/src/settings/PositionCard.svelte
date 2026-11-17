<script lang="ts">
  import type { Settings } from '$lib/ipc/generated/Settings';

  import Select from './Select.svelte';
  import SettingRow from './SettingRow.svelte';
  import SettingsCard from './SettingsCard.svelte';

  interface Props {
    settings: Settings;
    /** `SettingsStore.edit`: changes a copy of the document and saves it. */
    edit: (mutate: (draft: Settings) => void) => void;
  }

  let { settings, edit }: Props = $props();
</script>

<SettingsCard>
  <SettingRow id="position" label="Position" description="Edge of the screen the dock sits on">
    <Select
      id="position"
      value={settings.position}
      options={[
        { value: 'bottom', label: 'Bottom' },
        { value: 'left', label: 'Left' },
        { value: 'right', label: 'Right' },
        { value: 'top', label: 'Top' },
      ]}
      onchange={(v) =>
        edit((s) => {
          s.position = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="monitors"
    label="Monitors"
    description="Where the dock appears; a dock on every monitor follows plugging and unplugging"
  >
    <Select
      id="monitors"
      value={settings.monitors}
      options={[
        { value: 'primary', label: 'Primary monitor only' },
        { value: 'all', label: 'All monitors' },
      ]}
      onchange={(v) =>
        edit((s) => {
          s.monitors = v;
        })}
    />
  </SettingRow>
</SettingsCard>
