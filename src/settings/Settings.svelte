<script lang="ts">
  // Placeholder for the settings window. It only proves that the
  // second page of the multi-page app builds and mounts.
  const prefersDark = $state(
    typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: dark)').matches,
  );

  $effect(() => {
    document.documentElement.dataset.theme = prefersDark ? 'dark' : 'light';
  });
</script>

<main class="settings">
  <h1>Settings</h1>
  <p>The settings UI arrives later. Until then, edit <code>settings.json</code> directly.</p>
</main>

<style>
  .settings {
    box-sizing: border-box;
    min-height: 100%;
    padding: 24px 28px;
    color: var(--text);
    background: rgb(var(--panel-tint));
  }

  h1 {
    margin: 0 0 8px;
    font-size: 22px;
    font-weight: 600;
  }

  p {
    margin: 0;
    color: var(--text-muted);
  }

  code {
    font-family: 'Cascadia Code', Consolas, monospace;
    font-size: 12px;
  }
</style>
