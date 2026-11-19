import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  build: { outDir: 'dist', target: 'es2021', chunkSizeWarningLimit: 2000 },
  server: { port: 5173, strictPort: true, proxy: { '/api': 'http://127.0.0.1:7777' } },
})
