<script lang="ts">
  import type { Snippet } from 'svelte';

  // One row of a settings card: label and description on the left, bound to
  // the control on the right through `for={id}`.
  interface Props {
    /** The id of the control inside `children`. */
    id: string;
    label: string;
    description?: string;
    /** Greyed out; `tag` says why (the milestone that brings it). */
    disabled?: boolean;
    tag?: string;
    children: Snippet;
  }

  let { id, label, description, disabled = false, tag, children }: Props = $props();
</script>

<div class="row" class:disabled>
  <div class="text">
    <label for={id}>
      {label}
      {#if tag}
        <span class="tag" title="Planned for milestone {tag}">{tag}</span>
      {/if}
    </label>
    {#if description}
      <p class="description" id="{id}-description">{description}</p>
    {/if}
  </div>
  <div class="control">
    {@render children()}
  </div>
</div>

<style>
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 16px;
    min-height: 64px;
    box-sizing: border-box;
    padding: 12px 16px;
  }

  .row.disabled .text {
    color: var(--text-disabled);
  }

  .row.disabled .description {
    color: var(--text-disabled);
  }

  label {
    display: block;
    font-size: 14px;
    line-height: 20px;
  }

  .description {
    margin: 2px 0 0;
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 16px;
  }

  .tag {
    display: inline-block;
    margin-left: 6px;
    padding: 0 6px;
    border-radius: 9px;
    border: 1px solid var(--card-border);
    background: var(--control-bg);
    color: var(--text-secondary);
    font-size: 11px;
    line-height: 16px;
    font-weight: 600;
    vertical-align: 1px;
  }

  .control {
    display: flex;
    align-items: center;
    justify-content: flex-end;
  }
</style>
