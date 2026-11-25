import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';
import { inTauri } from './lib/api.ts';
import { installUiLog } from './lib/uilog.ts';

if (inTauri()) document.documentElement.dataset.native = '';

const target = document.getElementById('app');
if (!target) throw new Error('missing #app');

const app = mount(App, { target });
if (inTauri()) installUiLog(() => window.location.hash.replace(/^#\/?/, '').split('?')[0] || 'panel');
else void import('./lib/mock.ts').then((m) => m.bootMock());

export default app;
