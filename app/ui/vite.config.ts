import path from 'path';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';
import { viteSingleFile } from 'vite-plugin-singlefile';

// One self-contained index.html (no external files) → app/src, which Tauri serves.
export default defineConfig({
  plugins: [react(), viteSingleFile()],
  resolve: { alias: { '@': path.resolve(__dirname, './src') } },
  build: { outDir: '../src', emptyOutDir: false, chunkSizeWarningLimit: 2000 },
  server: { port: 1420, strictPort: true },
});
