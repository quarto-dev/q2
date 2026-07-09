import { defineConfig, type UserConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { viteSingleFile } from 'vite-plugin-singlefile';
import { copyFileSync } from 'fs';
import { resolve } from 'path';

// Build service worker or main app based on environment variable
const isServiceWorkerBuild = process.env.BUILD_TARGET === 'service-worker';

const serviceWorkerConfig: UserConfig = {
  plugins: [
    {
      name: 'copy-sw-to-parent',
      closeBundle() {
        const srcPath = resolve(__dirname, 'dist/serviceWorker.js');
        const destPath = resolve(__dirname, '../public/serviceWorker.js');
        try {
          copyFileSync(srcPath, destPath);
          console.log('✓ Copied service worker to ../public/serviceWorker.js');
        } catch (err) {
          console.error('Failed to copy service worker:', err);
        }
      },
    },
  ],
  build: {
    outDir: 'dist',
    emptyOutDir: false, // Don't clear dist when building SW
    lib: {
      entry: resolve(__dirname, 'src/serviceWorker.ts'),
      name: 'ServiceWorker',
      fileName: () => 'serviceWorker.js',
      formats: ['iife'],
    },
    rollupOptions: {
      output: {
        // Ensure no code splitting for service worker
        inlineDynamicImports: true,
      },
    },
  },
};

const mainConfig: UserConfig = {
  plugins: [
    react(),
    viteSingleFile(), // Bundle everything into a single HTML file
    {
      name: 'copy-to-parent',
      closeBundle() {
        const srcPath = resolve(__dirname, 'dist/index.html');
        const destPath = resolve(__dirname, '../public/q2-raw.html');
        try {
          copyFileSync(srcPath, destPath);
          console.log('✓ Copied to ../public/q2-raw.html');
        } catch (err) {
          console.error('Failed to copy to q2-raw.html:', err);
        }
      },
    },
  ],
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      input: 'index.html',
    },
  },
};

// https://vite.dev/config/
export default defineConfig(isServiceWorkerBuild ? serviceWorkerConfig : mainConfig);
