# Amp native resources

The native plugin metadata contract was inspected on 2026-10-29 against official
macOS arm64 CLI `0.0.1793217669-gb7b7f6`. This is separate from the earlier global
MCP/Skills writer audit in `data/agents.json`; no writer or codec changes here.

## Distribution and storage

The [official CLI guide](https://ampcode.com/docs/cli) links the installer, whose
source resolves a version before downloading its binary and SHA-256 from the
matching version directory at `static.ampcode.com/cli/`. The binary is 135,922,832
bytes, SHA-256 `9ac1809d4b5fe21093c0562c9d4be5a82cd8a7c67ab69dbd3db9e91ab51d5675`.
It matches the official checksum; Developer ID Application Amp Frontier
Corporation (`PZT9BJUAA5`) and strict codesign verification passed. Notarization
was not independently established. Inspection and installation did not execute
the target, native helper, plugin code, initializer or login flow.

The Bun graph contains a compiled JavaScript member and a keyring addon. Its
actual JavaScript storage is `amp!/$bunfs/root/amp-darwin-arm64`, an 18,132,898-byte
UTF-16LE member. Arca binds the complete native binary digest, including bytecode
and companion bytes, then exposes the audited version and shipped source marker.
A changed binary or unsupported wrapper supplies no version through an active
fallback probe. The digest is package identity evidence, not runtime observation.

The local CLI's callback references three internal factories: `permissions`,
`app-store-connect` and `thread-visibility`. They are shown together as shipped
Code markers. Authentication, permissions configuration, feature flags and
session membership are unobserved. No builtin MCP or Skill is inferred from
these factories or from documentation mentioning a plugin-writing Skill.

## Native discovery

The [official plugin guide](https://ampcode.com/docs/customize/plugins) describes
an executable TypeScript/JavaScript module API. The shipped loader confirms:

- Machine plugins use `XDG_CONFIG_HOME` when nonempty, otherwise `~/.config`,
  followed by `amp/plugins`. `AMP_HOME` is not this plugin root.
- Root `.ts`/`.js` files are candidates except `.test.ts`/`.test.js`. A root file
  masks a same-stem directory. Directory candidates select regular `index.ts`
  before regular `index.js`. Unsupported links/special entries stay references;
  they do not unlock a masked directory.
- Tools, commands, events, agent modes and Skills are registered by running code.
  Directory Skills require explicit `registerSkill` calls; `skills/` alone does
  not register them. Body validity, identical-source deduplication and runtime
  aliases cannot be established from unread filesystem metadata.
- Hosted personal/workspace packages are selected by an authenticated server
  response. The local cache uses nullish `XDG_CACHE_HOME` or `~/.cache`, followed
  by `amp/global-plugins/<sanitized-host>/<user|workspace>/<name>@<version>`.
  Cached directories remain cache-only, including older versions and packages
  from other servers. The observer does not guess their server-selected format,
  entrypoint, current account, body validity or active version.

An empty configuration override selects the default; an empty cache override
does not. Relative, tilde, traversal-bearing or unsupported root paths remain
incomplete without a project/default fallback. Local and cache IDs distinguish
filesystem snapshots and do not claim runtime registration identities.

## Boundaries

The observer reads no private plugin/Markdown body, account configuration or
credential. It does not initialize Amp, query account APIs or inspect project
plugins. Compiled markers, local candidates and cached references keep effective
enablement unknown and expose no delete, toggle, import or assignment action.
Existing central assets and global MCP/Skill writers retain their ownership.

Direct source inspection and REA cover the selected loader, PluginAPI, bootstrap
and cache functions. Five mapped analysis groups contain 90,279 source bytes;
all parsed with zero failures, while semantic coverage retains 5,562 unknowns.
Selected functions/assignments omit surrounding context and do not certify the
whole program or runtime activation. Embedded third-party SDK manifest strings
are not evidence that Amp's native plugins follow Agent Plugins 1.0.

Regression source covers passive identity, discovery precedence, unread bodies,
cache scopes/versions, unsafe entries, override semantics and watch paths. The
project's fast delivery mode retains these cases without running suites.
