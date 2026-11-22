import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// two pages: the main window and the guide that floats over Roblox while filming
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  build: { outDir: 'dist', target: 'es2021', chunkSizeWarningLimit: 2000, rollupOptions: { input: { main: 'index.html', overlay: 'overlay.html' } } },
  server: { port: 5174, strictPort: true, fs: { strict: false } },
})
