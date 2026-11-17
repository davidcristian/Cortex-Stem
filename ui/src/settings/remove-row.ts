import { tick } from 'svelte';

/**
 * Run `remove` for the row of `id` among the rows `rowSelector` matches
 * and, when the keyboard focus was in that row and went with it (to
 * `<body>`), put it on the same button (`buttonSelector`) of the row that
 * took its place, else of the row before it, else on the section's
 * heading (`headingId`, focusable by script only). A row that is
 * still there (the command failed) and a focus the user has moved
 * meanwhile are left alone.
 */
export async function removeRow(
  rowSelector: string,
  buttonSelector: string,
  headingId: string,
  id: string,
  remove: () => Promise<void>,
): Promise<void> {
  const rows = (): HTMLElement[] => Array.from(document.querySelectorAll<HTMLElement>(rowSelector));
  const before = rows();
  const index = before.findIndex((row) => row.dataset.id === id);
  const row = before[index];
  const focused = row !== undefined && row.contains(document.activeElement);
  await remove();
  await tick();
  const now = document.activeElement;
  if (!focused || row.isConnected || (now !== null && now !== document.body)) {
    return;
  }
  const after = rows();
  const next = after[index] ?? after[index - 1];
  (next?.querySelector<HTMLElement>(buttonSelector) ?? document.getElementById(headingId))?.focus();
}
