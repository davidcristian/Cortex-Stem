<script lang="ts">
  import type { Snippet } from 'svelte';

  // A Windows 11 "settings card" group: one bordered, rounded surface whose
  // rows are separated by hairlines. Rows are whatever the caller renders.
  interface Props {
    children: Snippet;
    /** Small print under the rows (e.g. what is not possible yet). */
    note?: string;
  }

  let { children, note }: Props = $props();
</script>

<div class="card">
  {@render children()}
  {#if note}
    <p class="note">{note}</p>
  {/if}
</div>

<style>
  .card {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--card-border);
    border-radius: 8px;
    background: var(--card-bg);
    overflow: hidden;
  }

  .card > :global(* + *) {
    border-top: 1px solid var(--divider);
  }

  .note {
    margin: 0;
    padding: 10px 16px;
    color: var(--text-secondary);
    font-size: 12px;
  }
</style>
