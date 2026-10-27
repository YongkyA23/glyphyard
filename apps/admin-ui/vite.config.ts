import react from '@vitejs/plugin-react'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: { '/api': 'http://localhost:8080' },
    // tokens.css lives in web/shared at the repo root
    fs: { allow: [fileURLToPath(new URL('../..', import.meta.url))] },
  },
  build: {
    outDir: '../server/static',
    emptyOutDir: true,
  },
})
