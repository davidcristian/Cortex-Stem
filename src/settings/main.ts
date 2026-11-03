import { mount } from 'svelte';

import '../app.css';
import Settings from './Settings.svelte';

const target = document.getElementById('app');
if (!target) {
  throw new Error('settings.html has no #app element');
}

mount(Settings, { target });
