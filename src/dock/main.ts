import { mount } from 'svelte';

import '../app.css';
import Dock from './Dock.svelte';

const target = document.getElementById('app');
if (!target) {
  throw new Error('dock.html has no #app element');
}

mount(Dock, { target });
