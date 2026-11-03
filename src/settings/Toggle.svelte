<script lang="ts">
  // Windows 11 toggle switch: the state word to its left, the switch at the
  // right edge of the row. A native checkbox with `role="switch"`, so Space
  // toggles it and the row's label activates it.
  interface Props {
    id: string;
    checked: boolean;
    disabled?: boolean;
    onchange: (checked: boolean) => void;
  }

  let { id, checked, disabled = false, onchange }: Props = $props();
</script>

<span class="toggle" class:disabled>
  <span class="state" aria-hidden="true">{checked ? 'On' : 'Off'}</span>
  <input
    type="checkbox"
    role="switch"
    {id}
    {checked}
    {disabled}
    aria-checked={checked}
    onchange={(e) => onchange(e.currentTarget.checked)}
  />
</span>

<style>
  .toggle {
    display: inline-flex;
    align-items: center;
    gap: 12px;
  }

  .state {
    min-width: 24px;
    color: var(--text);
    font-size: 14px;
    text-align: right;
  }

  .disabled .state {
    color: var(--text-disabled);
  }

  input {
    appearance: none;
    -webkit-appearance: none;
    position: relative;
    box-sizing: border-box;
    width: 40px;
    height: 20px;
    margin: 0;
    border: 1px solid var(--switch-border);
    border-radius: 10px;
    background: var(--switch-bg);
    cursor: default;
    transition:
      background 120ms ease,
      border-color 120ms ease;
  }

  input::before {
    content: '';
    position: absolute;
    top: 3px;
    left: 3px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--switch-knob);
    transition:
      transform 160ms cubic-bezier(0.2, 0.8, 0.2, 1),
      width 120ms ease,
      background 120ms ease;
  }

  input:hover:not(:disabled)::before {
    width: 14px;
  }

  input:checked {
    border-color: var(--accent);
    background: var(--accent);
  }

  input:checked::before {
    background: #fff;
    transform: translateX(20px);
  }

  input:checked:hover:not(:disabled)::before {
    transform: translateX(18px);
  }

  input:disabled {
    opacity: 0.45;
  }

  input:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 2px;
  }

  @media (prefers-reduced-motion: reduce) {
    input,
    input::before {
      transition: none;
    }
  }
</style>
