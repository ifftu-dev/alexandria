import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'
import vueI18n from '@intlify/unplugin-vue-i18n/vite'
import { resolve } from 'path'

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    vue(),
    tailwindcss(),
    // Precompile locale catalogs to render functions at build time.
    // The Tauri webview CSP is `script-src 'self'` (no unsafe-eval), so the
    // vue-i18n *runtime* message compiler (which uses `new Function`) would
    // throw. `runtimeOnly` drops that compiler; `compositionOnly` tree-shakes
    // the legacy API.
    vueI18n({
      // Only the JSON namespace files are message catalogs. The `.ts` barrels
      // and `meta.ts` are normal modules — including them makes the plugin try
      // to precompile them as locale resources.
      include: [resolve(__dirname, 'src/locales/**/*.json')],
      runtimeOnly: true,
      compositionOnly: true,
      strictMessage: false,
    }),
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
