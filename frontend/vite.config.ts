import vue from '@vitejs/plugin-vue'
import ui from '@nuxt/ui/vite'
import { defineConfig } from 'vite'

export default defineConfig({
  // The page is served under /admin in production; keep the same path in dev.
  base: '/admin/',
  plugins: [
    vue(),
    ui({
      // Standalone Vue app: no router, light theme only.
      router: false,
      colorMode: false,
      ui: {
        colors: {
          primary: 'blue',
          neutral: 'slate',
        },
      },
    }),
  ],
  server: {
    proxy: {
      '/admin/api': 'http://127.0.0.1:8080',
      '/config': 'http://127.0.0.1:8080',
      '/files': 'http://127.0.0.1:8080',
    },
  },
})
