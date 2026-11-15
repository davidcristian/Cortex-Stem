import { mount } from 'svelte';

import { createBackend, MockBackend } from '$lib/ipc';
import { applyDemoQuery, exposeMock } from '$lib/ipc/demo';
import { DockStore } from '$lib/stores/dock.svelte';

import '../app.css';
import Dock from './Dock.svelte';

const target = document.getElementById('app');
if (!target) {
  throw new Error('dock.html has no #app element');
}

const backend = createBackend();
const demo = backend instanceof MockBackend;
if (demo) {
  // `?position=` and the like, before the store asks for anything.
  applyDemoQuery(backend, window.location.search);
  exposeMock(backend);
}

mount(Dock, { target, props: { store: new DockStore(backend), demo } });
