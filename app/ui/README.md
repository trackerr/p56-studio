# P56 Studio UI

React + TypeScript + Vite, built into one self-contained file (`../src/index.html`) that the Tauri app
and `p56-server` both serve.

```bash
pnpm install
pnpm dev          # http://localhost:1420, with a demo fan when not running inside the app
pnpm exec tsc -b  # type check
pnpm run bundle   # → ../src/index.html (commit it with UI changes)
```

- `src/App.tsx`: shell (header, orbit rail / bottom bar, tabs)
- `src/panels/`: one file per tab
- `src/components/bits.tsx`: the design's own parts (`Orbit`, `Choice`, `Section`, …)
- `src/components/icons.tsx`: the app's icons
- `src/components/ui/`: Radix primitives with the app's styling
- `src/lib/api.ts`: bridge to the Rust backend (Tauri, `p56-server`, or the in-browser demo fan)
- `src/lib/i18n.tsx`: English and Dutch texts

The design (tokens, components, layout) is described in [docs/DESIGN.md](../../docs/DESIGN.md).
