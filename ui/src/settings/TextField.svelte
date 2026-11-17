<script lang="ts">
  // Windows 11 text box. Typing only edits a local draft; the trimmed value
  // is committed on Enter or when the box loses focus, and Escape puts the
  // committed value back, so a half-typed value is never applied.
  interface Props {
    id: string;
    value: string;
    placeholder?: string;
    /** Id of the element that describes the box (the row's description). */
    describedby?: string;
    disabled?: boolean;
    oncommit: (value: string) => void;
  }

  let { id, value, placeholder, describedby, disabled = false, oncommit }: Props = $props();

  // Follows `value`; typing overrides it until the next commit or revert.
  let draft = $derived(value);

  function commit(): void {
    const next = draft.trim();
    if (next === value) {
      draft = value;
      return;
    }
    oncommit(next);
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter') {
      e.preventDefault();
      commit();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      draft = value;
    }
  }
</script>

<input
  type="text"
  {id}
  value={draft}
  {placeholder}
  {disabled}
  aria-describedby={describedby}
  autocomplete="off"
  spellcheck="false"
  oninput={(e) => (draft = e.currentTarget.value)}
  onblur={commit}
  {onkeydown}
/>

<style>
  input {
    box-sizing: border-box;
    width: 200px;
    max-width: 100%;
    height: 32px;
    margin: 0;
    padding: 0 11px;
    border: 1px solid var(--control-border);
    border-bottom-color: var(--control-border-bottom);
    border-radius: 4px;
    background: var(--control-bg);
    color: var(--text);
    font: inherit;
    font-size: 14px;
  }

  input::placeholder {
    color: var(--text-secondary);
  }

  input:hover:not(:disabled):not(:focus) {
    background: var(--control-bg-hover);
  }

  /* The Windows 11 focus look: an accent underline instead of a ring. */
  input:focus {
    outline: none;
    border-bottom: 2px solid var(--accent);
  }

  input:disabled {
    color: var(--text-disabled);
    background: var(--control-bg-disabled);
  }
</style>
