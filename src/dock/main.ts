import { mount } from 'svelte';

import { createBackend, MockBackend } from '$lib/ipc';
import { DockStore } from '$lib/stores/dock.svelte';

import '../app.css';
import Dock from './Dock.svelte';

declare global {
  interface Window {
    /** Browser demo only: script the mock from the devtools console. */
    __dockMock?: MockBackend;
  }
}

const target = document.getElementById('app');
if (!target) {
  throw new Error('dock.html has no #app element');
}

const backend = createBackend();
const demo = backend instanceof MockBackend;
if (demo) {
  window.__dockMock = backend;
}

mount(Dock, { target, props: { store: new DockStore(backend), demo } });
