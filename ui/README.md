# Zaklon interface

The React interface (TypeScript, Vite) shared by the desktop window and the Android app. The hub serves the built files from `ui/dist` (a debug hub reads them from disk, a release build embeds them), so the usual loop is: change the code, run `pnpm --filter ui build`, reload the page.

Run from the repository root:

| Command | What it does |
|---|---|
| `pnpm --filter ui dev` | Vite development server with live reload, for layout work (the interface expects the hub on the same address, so screens show no data) |
| `pnpm --filter ui build` | Type check and production build into `ui/dist` |
| `pnpm --filter ui lint` | Lint with oxlint |
| `pnpm --filter ui e2e` | Playwright end-to-end tests against a real hub (build it first with `cargo build -p zaklon-hub`) |

Translations live in `src/i18n.ts` (English and Serbian). Tests and their setup are described in [docs/TESTING.md](../docs/TESTING.md); building the whole app is described in [CONTRIBUTING.md](../CONTRIBUTING.md#building-from-source).
