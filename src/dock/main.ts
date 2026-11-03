import { mount } from 'svelte';

import { createBackend, MockBackend } from '$lib/ipc';
import { exposeMock } from '$lib/ipc/demo';
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
  exposeMock(backend);
}

mount(Dock, { target, props: { store: new DockStore(backend), demo } });
