# Arca official website

VitePress documentation and product media for <https://mux.scoheart.com>. Chinese and English pages cover the same released capabilities. The deployment is the existing Vercel `website` project with this directory as its root.

## Develop and build

```bash
npm ci
npm run dev -- --host 0.0.0.0
npm run build
```

Check navigation, both languages, narrow screens, reference-table filters, media playback, captions, and chapter seeking before deployment. The player loads no video until requested (`preload="none"`) and has no autoplay.

## Update public references

After an app release, use that exact version's verified bundled or standalone CLI:

```bash
node scripts/update-reference.mjs --arca /absolute/path/to/arca
```

The script checks CLI/source version agreement, reads audited Agent names and paths from `data/agents.json`, and obtains Model capabilities and Provider templates from Core. It creates a temporary `ARCA_HOME`, publishes only selected public contract fields, rejects private absolute paths, and writes `.vitepress/reference.json`. This generated snapshot keeps the site independently buildable on Vercel without a Rust toolchain. Do not hand-edit names, counts, or capability lists in pages.

## Product media

`public/media/` contains reviewed public derivatives of the 1.10.0 release UI: a desktop tour, four screenshots, and Chinese / English captions. Recording uses a separate macOS bundle identity with a local ad-hoc signature so automation cannot select the installed app by mistake; product code stays the same. It uses synthetic assets and illustrative endpoints rather than user configuration or conversations. No live model request is recorded. Original recordings and sample data stay in the local ignored artifact directory.

When replacing the tour, update its versioned filenames, poster, captions, and chapter times together in `ArcaDemo.vue`. Keep visible playback controls, inline mobile playback, captions, and a download link. Record the actual app UI; an animated mockup is not release evidence.

Website changes are independent of app release version generation. See the repository [AGENTS.md](../AGENTS.md) for delivery rules.
