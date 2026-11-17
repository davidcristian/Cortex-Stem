<script lang="ts" generics="T extends string">
  // Windows 11 combo box look on a native <select>: keyboard, screen readers
  // and the OS popup come for free.
  interface Option<T> {
    value: T;
    label: string;
    disabled?: boolean;
  }

  interface Props {
    id: string;
    value: T;
    options: Option<T>[];
    disabled?: boolean;
    onchange: (value: T) => void;
  }

  let { id, value, options, disabled = false, onchange }: Props = $props();
</script>

<select {id} {value} {disabled} onchange={(e) => onchange(e.currentTarget.value as T)}>
  {#each options as option (option.value)}
    <option value={option.value} disabled={option.disabled}>{option.label}</option>
  {/each}
</select>

<style>
  select {
    appearance: none;
    -webkit-appearance: none;
    box-sizing: border-box;
    /* Fixed like a Windows 11 ComboBox; a long option never widens the box. */
    width: 288px;
    max-width: 100%;
    height: 32px;
    padding: 0 34px 0 11px;
    border: 1px solid var(--control-border);
    border-bottom-color: var(--control-border-bottom);
    border-radius: 4px;
    background-color: var(--control-bg);
    background-image: var(--chevron);
    background-repeat: no-repeat;
    background-position: right 11px center;
    background-size: 12px 12px;
    color: var(--text);
    font: inherit;
    font-size: 14px;
    cursor: default;
  }

  select:hover:not(:disabled) {
    background-color: var(--control-bg-hover);
  }

  select:disabled {
    color: var(--text-disabled);
    background-color: var(--control-bg-disabled);
    border-color: var(--control-border);
  }

  select:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 1px;
  }

  option {
    color: var(--text);
    background: var(--card-bg);
  }
</style>
