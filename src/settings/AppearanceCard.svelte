<script lang="ts">
  import type { Snippet } from 'svelte';

  import type { Settings } from '$lib/ipc/generated/Settings';

  import { icons, percent, px, scale } from './format';
  import Select from './Select.svelte';
  import SettingRow from './SettingRow.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import Slider from './Slider.svelte';
  import Toggle from './Toggle.svelte';

  interface Props {
    settings: Settings;
    /** `SettingsStore.edit`: changes a copy of the document and saves it. */
    edit: (mutate: (draft: Settings) => void) => void;
    /** The rows after the colour mode: the page's dock.css row. */
    children: Snippet;
  }

  let { settings, edit, children }: Props = $props();
</script>

<SettingsCard>
  <SettingRow id="icon-size" label="Icon size" description="Size of the icons at rest">
    <Slider
      id="icon-size"
      min={24}
      max={128}
      step={1}
      value={settings.iconSize}
      format={px}
      onchange={(v) =>
        edit((s) => {
          s.iconSize = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="magnification"
    label="Magnification"
    description="Icons grow as the pointer approaches them"
  >
    <Toggle
      id="magnification"
      checked={settings.magnification.enabled}
      onchange={(v) =>
        edit((s) => {
          s.magnification.enabled = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="magnification-scale"
    label="Maximum scale"
    description="Size of the icon under the pointer"
    disabled={!settings.magnification.enabled}
  >
    <Slider
      id="magnification-scale"
      min={1}
      max={2.5}
      step={0.05}
      value={settings.magnification.maxScale}
      disabled={!settings.magnification.enabled}
      format={scale}
      onchange={(v) =>
        edit((s) => {
          s.magnification.maxScale = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="magnification-range"
    label="Range"
    description="Neighbouring icons on each side that grow too"
    disabled={!settings.magnification.enabled}
  >
    <Slider
      id="magnification-range"
      min={1}
      max={6}
      step={1}
      value={settings.magnification.rangeIcons}
      disabled={!settings.magnification.enabled}
      format={icons}
      onchange={(v) =>
        edit((s) => {
          s.magnification.rangeIcons = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="appearance-theme"
    label="Theme"
    description="How the panel is drawn: translucent, opaque or icons only. Your dock.css, further down, can change any of them."
  >
    <Select
      id="appearance-theme"
      value={settings.appearance.theme}
      options={[
        { value: 'standard', label: 'Standard (translucent)' },
        { value: 'solid', label: 'Solid (opaque)' },
        { value: 'minimal', label: 'Minimal (icons only)' },
      ]}
      onchange={(v) =>
        edit((s) => {
          s.appearance.theme = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="opacity"
    label="Panel opacity"
    description={settings.appearance.theme === 'standard'
      ? undefined
      : 'Only the Standard theme is translucent'}
    disabled={settings.appearance.theme !== 'standard'}
  >
    <Slider
      id="opacity"
      min={0}
      max={1}
      step={0.01}
      value={settings.appearance.opacity}
      disabled={settings.appearance.theme !== 'standard'}
      format={percent}
      onchange={(v) =>
        edit((s) => {
          s.appearance.opacity = v;
        })}
    />
  </SettingRow>
  <SettingRow id="corner-radius" label="Corner radius">
    <Slider
      id="corner-radius"
      min={0}
      max={64}
      step={1}
      value={settings.appearance.cornerRadius}
      format={px}
      onchange={(v) =>
        edit((s) => {
          s.appearance.cornerRadius = v;
        })}
    />
  </SettingRow>
  <!-- Only the built-in blur is implemented, not the system
       materials. The other two stay as options so a settings
       file that names one still shows its value, but cannot be chosen. -->
  <SettingRow
    id="backdrop"
    label="Backdrop"
    description="How the panel blends with what is behind it. Acrylic and Mica are not available yet."
  >
    <Select
      id="backdrop"
      value={settings.appearance.backdrop}
      options={[
        { value: 'css', label: 'Blur (built in)' },
        { value: 'acrylic', label: 'Acrylic (not yet available)', disabled: true },
        { value: 'mica', label: 'Mica (not yet available)', disabled: true },
      ]}
      onchange={(v) =>
        edit((s) => {
          s.appearance.backdrop = v;
        })}
    />
  </SettingRow>
  <SettingRow id="tooltips" label="Tooltips" description="Show app names on hover">
    <Toggle
      id="tooltips"
      checked={settings.appearance.tooltips}
      onchange={(v) =>
        edit((s) => {
          s.appearance.tooltips = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="bounce"
    label="Bounce on launch"
    description="Animate the icon while an app starts"
  >
    <Toggle
      id="bounce"
      checked={settings.appearance.bounceOnLaunch}
      onchange={(v) =>
        edit((s) => {
          s.appearance.bounceOnLaunch = v;
        })}
    />
  </SettingRow>
  <SettingRow
    id="theme"
    label="Colour mode"
    description="Light or dark colours for the dock and this window"
  >
    <Select
      id="theme"
      value={settings.theme}
      options={[
        { value: 'system', label: 'Use Windows setting' },
        { value: 'light', label: 'Light' },
        { value: 'dark', label: 'Dark' },
      ]}
      onchange={(v) =>
        edit((s) => {
          s.theme = v;
        })}
    />
  </SettingRow>
  {@render children()}
</SettingsCard>
