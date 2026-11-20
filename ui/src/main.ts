import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';
import { inTauri } from './lib/api.ts';
import { bootMock } from './lib/mock.ts';
import { installUiLog } from './lib/uilog.ts';

const target = document.getElementById('app');
if (!target) throw new Error('missing #app');

const app = mount(App, { target });
if (inTauri()) installUiLog(() => window.location.hash.replace(/^#\/?/, '').split('?')[0] || 'panel');
else bootMock();

export default app;
