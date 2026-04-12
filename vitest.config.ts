import { fileURLToPath, URL } from 'node:url'
import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'

/**
 * Test runner for Vue 3 composables + components.
 *
 * - jsdom for DOM APIs (matchMedia, localStorage, etc.)
 * - `@/` alias mirrors `vite.config.ts`
 * - `@tauri-apps/api/core` is mocked by tests that need it (see
 *   `src/composables/__tests__/useOmniSearch.test.ts`); no global mock
 *   here so non-Tauri composables stay testable.
 */
export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    include: ['src/**/*.{test,spec}.ts'],
    // Keep tests in sync with strict mode: any unused imports or
    // locals in a test file should fail fast rather than silently drift.
    typecheck: {
      enabled: false, // vue-tsc in CI handles type checking
    },
  },
})
