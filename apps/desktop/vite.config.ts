import tailwindcss from '@tailwindcss/vite';
import { bootScript } from '@uwusuite/design';
import react from '@vitejs/plugin-react';
import { defineConfig, type Plugin } from 'vite';
import pkg from './package.json' with { type: 'json' };

/**
 * The theme, contrast and motion go onto <html> before the first paint, so a
 * dark window never flashes white (@uwusuite/design's bootScript). It reads
 * the window's preferences from localStorage (lib/prefs.ts). A file, boot.js,
 * not inline, so a CSP with script-src 'self' allows it.
 */
function boot(): Plugin {
  const source = bootScript('uwupulse.settings');
  return {
    name: 'uwupulse-boot',
    configureServer(server) {
      server.middlewares.use('/boot.js', (_request, response) => {
        response.setHeader('content-type', 'text/javascript');
        response.end(source);
      });
    },
    generateBundle() {
      this.emitFile({ type: 'asset', fileName: 'boot.js', source });
    },
  };
}

// Tauri drives the dev server, so the port is fixed and failures must be loud.
export default defineConfig({
  plugins: [react(), tailwindcss(), boot()],
  clearScreen: false,
  // Relative asset URLs: the same build runs in Tauri and under the core's /ui/.
  base: './',
  // The version for the About and Updates cards (lib/prefs.ts).
  define: {
    'import.meta.env.APP_VERSION': JSON.stringify(pkg.version),
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ['**/src-tauri/**', '**/target/**'],
    },
  },
  build: {
    target: 'chrome110',
    sourcemap: false,
  },
});
