import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

// `vitest/config` is vite's `defineConfig` with the test runner's settings typed.
// The bundle ships everything it needs: no external asset is fetched at runtime,
// so the interface renders with no network.
export default defineConfig({
  plugins: [react()],
  build: { target: 'es2022', sourcemap: false, assetsInlineLimit: 0 },
  test: { environment: 'node', globals: true, include: ['src/**/*.test.ts?(x)'] },
});
