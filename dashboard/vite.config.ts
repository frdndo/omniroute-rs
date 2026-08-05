import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    // Gagal KERAS kalau port kepake — jangan auto-pindah ke 5174
    // (devUrl Tauri tetap nunjuk 5173 → kalau pindah, webview blank)
    strictPort: true,
    watch: {
      // jangan re-compile saat build tools nulis file (target dirs)
      ignored: ['**/src-tauri/**', '**/rust-core/**', '**/target/**'],
    },
  },
})
