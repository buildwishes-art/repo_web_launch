import { mount } from 'svelte';
import App from './App.svelte';
import './app.css';

// Mode pengembangan UI di browser biasa (tanpa Tauri): pakai IPC tiruan.
if (import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)) {
  (await import('./dev-mock')).installMock();
}

const app = mount(App, { target: document.getElementById('app')! });

export default app;
