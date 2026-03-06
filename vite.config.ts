import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'
import { resolve } from 'path'

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    vue(),
    tailwindcss(),
  ],
  resolve: {
    alias: {
      '@': resolve(__dirname, 'src'),
    },
  },
  // Tauri expects a fixed port in dev mode
  server: {
    host: process.env.TAURI_DEV_HOST || 'localhost',
    port: 5173,
    strictPort: true,
    watch: {
      // Exclude cargo build directory to avoid ELOOP from symlinks
      ignored: ['**/target/**'],
    },
  },
  // Prevent Vite from obscuring Rust errors
  clearScreen: false,
  // Tauri needs to know the dev server URL for the webview
  envPrefix: ['VITE_', 'TAURI_'],
})
