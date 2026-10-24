import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('.', import.meta.url));

// Three HTML entries (overlay, main, area) share src/lib. Tauri loads them by URL.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, host: '127.0.0.1' },
  build: {
    target: 'safari15',
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      input: {
        overlay: resolve(root, 'overlay.html'),
        main: resolve(root, 'main.html'),
        area: resolve(root, 'area.html'),
      },
    },
  },
  test: { include: ['src/**/*.test.ts'], environment: 'node' },
});
