import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// The bundle ships everything it needs: no external asset is fetched at runtime,
// so the interface renders with no network.
export default defineConfig({
  plugins: [react()],
  build: { target: 'es2022', sourcemap: false, assetsInlineLimit: 0 },
  test: { environment: 'node', globals: true, include: ['src/**/*.test.ts?(x)'] },
});
