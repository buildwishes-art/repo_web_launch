import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Konfigurasi untuk Tauri: port tetap (dipakai tauri.conf.json → devUrl) dan target WebView2 (Chromium).
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: '127.0.0.1',
    watch: { ignored: ['**/src-tauri/**'] },
  },
  build: {
    target: 'chrome110',
    minify: 'esbuild',
    sourcemap: false,
  },
});
