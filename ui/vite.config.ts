import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  build: {
    // Some computers keep an old WebView2 (updates blocked): write CSS and JS
    // that Chromium 90 still understands.
    target: ['es2020', 'chrome90'],
    cssTarget: ['chrome90'],
  },
})
