# Arca Model playground

This is Arca's local browser workbench for comparing text and reasoning models.
It uses React, TypeScript and Vite. The browser keeps the current prompt,
comparison settings and recent rounds; Arca Core reads saved Model Profiles and
sends each request to its Provider using the configured credential. Each request
can stream through the local connection or wait for a complete reply. Use the
request preview to check the protocol JSON before sending, and use the round
controls to retry a request, restore settings, or download the result as JSON.
Recent rounds live in the open browser tab.

Build and open the workbench from the Arca repository root:

```bash
npm --prefix playground ci
npm --prefix playground run build
cargo run --locked -p arca-cli -- model playground
```

The Rust build embeds the Vite output. The production panel and its API use the
same loopback origin; it needs no Node server at runtime. Desktop packaging
builds the frontend automatically. For desktop development, install the
playground's dependencies before starting `npm run tauri dev` from `desktop/`.

`npm --prefix playground run dev` starts Vite for frontend work. To open the
panel with saved models, use the Arca launch commands above.

- `src/App.tsx` manages draft requests and settings.
- `src/useRounds.ts` manages the live requests and recent round results.
- `src/components/` renders the run settings and results.
- `src/api.ts` handles the local API and serializes request parameters.
