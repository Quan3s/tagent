import { defineConfig } from 'vite';
import { svelte, vitePreprocess } from '@sveltejs/vite-plugin-svelte';

const domain = process.env.RAILWAY_PUBLIC_DOMAIN;
const allowedHosts = [domain].filter(Boolean);
const hmr = domain ? { protocol: 'wss', host: domain, clientPort: 443 } : undefined;

export default defineConfig({
  plugins: [svelte({ preprocess: vitePreprocess() })],
  server: { host: '0.0.0.0', port: 8080, strictPort: true, allowedHosts, hmr },
  preview: { host: '0.0.0.0', port: 8080, strictPort: true, allowedHosts },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'es2020',
    cssCodeSplit: true,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes('/node_modules/svelte/')) return 'svelte';
          return undefined;
        },
      },
    },
  },
});
