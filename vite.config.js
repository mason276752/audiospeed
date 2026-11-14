import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  base: '/audiospeed/',
  // 變速跑在 module worker 裡（見 src/stretch/）。
  worker: { format: 'es' },
  test: {
    include: ['src/**/*.test.mjs'],
  },
})
