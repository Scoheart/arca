# Arca repository instructions

- Arca currently manages global Agent MCP configurations only. Do not reintroduce project-scope UI, CLI flags, or writes unless an issue explicitly changes that product boundary.
- Preserve unrelated top-level keys, sibling MCP servers, and unmodelled fields when editing Agent JSON or TOML files. Formatting and key order may change after serialization.
- Keep the stable CLI release filename `arca_v<version>_<target-triple>.tar.gz`; released `arca upgrade` clients resolve that exact pattern.
- Do not edit release-owned versions in feature commits. `version.txt`, generated lock metadata, CHANGELOG, and the stable tag are advanced by the automatic release commit after a validated direct `main` push.
- Keep npm lockfiles committed and use `npm ci`. Stable publication keeps immutable-tag provenance, version, signing, App/DMG, updater, CLI, asset-set, and latest-channel checks. Automated Quality is paused. Never replace full-SHA Action pins with mutable tags.
- Preserve semantic-version latest selection for Stable publication; a slower older build must never replace a newer updater channel.
- Do not edit generated `dist/`, `target/`, or `.vitepress/dist/` output.
- Fast delivery mode is active: unless the current task explicitly requests validation, do not run tests, lint, formatting checks, icon checks, changed-surface validation, or push preflight. Inspect the diff, commit, and push `main` directly so Direct Stable can publish the next patch.
- Never merge a repair PR, move a stable tag, publish a Draft Release manually, or replace the installed App. Leave repair PRs ready for owner review.
