import react from '@vitejs/plugin-react'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'

// Tauri expects a fixed dev port; src-tauri points devUrl at http://localhost:1420 and frontendDist at ../ui/dist.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  server: {
    port: 1420,
    strictPort: true,
    // tokens.css lives in web/shared at the repo root
    fs: { allow: [fileURLToPath(new URL('../../..', import.meta.url))] },
  },
  build: { target: 'es2022', outDir: 'dist', emptyOutDir: true },
})
