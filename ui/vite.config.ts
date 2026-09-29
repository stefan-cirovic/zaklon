import react from '@vitejs/plugin-react'
import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'

const here = dirname(fileURLToPath(import.meta.url))

/** The version of a package the interface is built with, as installed from the lock file. */
function installed(name: string): string {
  try {
    return JSON.parse(readFileSync(join(here, 'node_modules', name, 'package.json'), 'utf8')).version ?? ''
  } catch {
    return ''
  }
}

/**
 * The commit and the day of this build, for the system specification
 * (Settings › About): ZAKLON_BUILD_COMMIT and ZAKLON_BUILD_DATE when set
 * (scripts/build-all.sh and the release workflow set them), else git and today.
 */
function build(): { commit: string; date: string } {
  let commit = process.env.ZAKLON_BUILD_COMMIT?.trim() ?? ''
  if (!/^[0-9a-f]+$/i.test(commit)) {
    try {
      commit = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: here, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim()
    } catch {
      commit = ''
    }
  }
  const given = process.env.ZAKLON_BUILD_DATE?.trim() ?? ''
  const date = /^\d{4}-\d{2}-\d{2}$/.test(given) ? given : new Date().toISOString().slice(0, 10)
  return { commit: /^[0-9a-f]+$/i.test(commit) ? commit.slice(0, 12).toLowerCase() : '', date }
}

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  define: {
    // What the interface is built with and when (see src/components/SystemSpec.tsx).
    __ZAKLON_BUILD__: JSON.stringify({
      ...build(),
      typescript: installed('typescript'),
      react: installed('react'),
      maplibre: installed('maplibre-gl'),
      basemaps: installed('@protomaps/basemaps'),
    }),
  },
  build: {
    // Some computers keep an old WebView2 (updates blocked): write CSS and JS
    // that Chromium 90 still understands.
    target: ['es2020', 'chrome90'],
    cssTarget: ['chrome90'],
  },
})
