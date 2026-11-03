import { mount } from 'svelte';

import { createBackend, MockBackend } from '$lib/ipc';
import { exposeMock } from '$lib/ipc/demo';
import { SettingsStore } from '$lib/stores/settings.svelte';

import '../app.css';
import Settings from './Settings.svelte';

const target = document.getElementById('app');
if (!target) {
  throw new Error('settings.html has no #app element');
}

const backend = createBackend();
if (backend instanceof MockBackend) {
  exposeMock(backend);
}

mount(Settings, { target, props: { store: new SettingsStore(backend) } });
