# Factory Desktop native plugin observation

## Audited artifact

The inspected official Apple Silicon package is Factory Desktop `0.208.0`,
bundle `com.electron.factory`, build `14897`. Its embedded Droid is `0.251.0`;
the Desktop and CLI version numbers identify different artifacts.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `Factory-0.208.0-arm64.dmg` | 269597671 | `18b416a4d78bfe33df6308f160f71c28894f6f73bb95e6db884b44e201e3706d` |
| `Contents/Resources/app.asar` | 64207723 | `4c8a57bf4782c7d2e1caa9a1970bb134a2167ec28f6e9509460f644545d234f4` |
| `Contents/Resources/bin/droid` | 292301504 | `7cf155967a78b75a47de53c5ed99522b77c22a4abd1617e74f167d882c059cfa` |

The [official Desktop page](https://factory.com/product/desktop) uses
`https://app.factory.ai/api/desktop?platform=darwin&architecture=arm64`, which
redirected to the inspected version under
`https://downloads.factory.ai/factory-desktop/releases/0.208.0/darwin/arm64/`.
The downloaded byte length matched the official response. No independently
published DMG SHA-256 was verified. No installer, app, daemon or native addon
was executed during source inspection.

Deep strict `codesign` verification of the complete readonly mounted app
passed. Its signer is The San Francisco AI Factory Inc. (`SW6TL4V6Q5`), and
signature metadata reports a stapled notarization ticket. This does not resolve
the separate ASAR contradiction below or establish native runtime behavior.

## Desktop and daemon source chain

`package.json` selects `.vite/build/main.js`, which requires
`.vite/build/index-DlQW8R5W.js`. The following packed modules bind the Desktop
composition, HOME selection and daemon launch:

- `index-DlQW8R5W.js` (`6a3255afb994d9c142a03e473873d1ef3de0cecccfb3430a9f6f0b1fb72144f3`)
  contains `V5`/`getDesktopHomeDir`, the production build configuration and
  the common daemon client/plugin RPC methods.
- `initLifecycle-D3kvsJfz.js` (`1d4b40a1cb6dcc9d7772977ffaf10c6ea53d2bb0dec50f2b41fd8cf783e1c6cc`)
  selects the packaged `Resources/bin/droid`, starts its `daemon` entry and
  supplies that same executable as `--droid-path`.
- Its launch configuration passes `FACTORY_HOME_OVERRIDE` from Desktop's HOME
  resolver and explicitly overwrites `FACTORY_ENV` and `FACTORY_DEPLOYMENT_ENV`
  with `production`. It also disables daemon auto-update.

Desktop's resolver prefers a nonempty `FACTORY_HOME_OVERRIDE`, then Electron's
`app.getPath("home")`, with `os.homedir()` as the non-Electron fallback. Arca's
passive global candidate uses its current HOME or home fallback and that
nonempty override, followed by `.factory`; it rejects unsafe/unresolved bases.
It does not inherit the CLI observer's `.factory-dev` selection. A running
Desktop may have a different inherited environment; its effective HOME and
current session state remain unknown without runtime observation.

The embedded Droid's complete Bun table has 878 members. Its plugin loader
`/$bunfs/root/chunk-4nb7ztsk.js` is byte-identical to the standalone audited
Droid loader (`892697a3879f45e79c3994d8deda16fe46402c888d0fd7b11556d055afc683a5`).
61 of the earlier plugin/schema source projections match complete member and
projection hashes. A renamed version module retains the exact `0.251.0`
function; the compiled configuration's only changed property is
`autoUpdateEnabled: true` to `false`. These are component comparisons, not a
claim that the complete binaries are interchangeable.

## Arca observation boundary

Factory's existing Desktop Agent keeps its independent `factory-desktop` ID.
The new observer selects `/Applications/Factory.app` before
`~/Applications/Factory.app`, binds Info.plist identity/version, and verifies
the complete ASAR and embedded daemon against the separate Desktop contract.
A present but invalid selected app blocks fallback to another copy or the
standalone CLI. The two public files have a separate finite verification
budget; their file identities are rechecked across the complete gate.

Only after that gate does it reuse the proven
[Droid registry and component grammar](factory-droid.md): strict BOM-aware
installation records, sorted scope overlays, registered absolute package
paths, portable restrictions, filename-only component candidates and validated
MCP metadata. Project registrations, arbitrary cache history, manifests and
marketplace checkouts do not establish global installations. Desktop and CLI
observations of shared physical registrations retain distinct host identities.
Watchers include both package guards and the native registry inputs; they
enumerate registered package roots only after successful runtime binding.

This adds no built-in MCP catalog, activation claim, login/credential handling,
writer, import, deletion or asset lifecycle operation. Component bodies,
settings/account files and authentication values are not exposed. Installed
resources remain read-only; enabled/effective/runtime activity stays unknown.
Future package versions require new source evidence and contract updates.

## Evidence and limitations

The DMG inventory retains Evidence
`ev_09e27b8e56616a0bafde647b07d264a5d1a0de1ffc681b66ecde80d30be36d8c`
and nested inventory
`ev_27741910ced911335cc781222a521ae6115538354184685aa0b31ac9fe648ad4`.
Its full exported bundle was completely JSON-parsed and SHA-256 checked.

Focused REA analysis of 20 exact Desktop declarations/declarators covers
33,893 bytes and 7,360 AST nodes, with no parse failures. Parent Evidence is
`ev_46dcb7096db9ab1dd7d3c0c53ff383e2739207127a147b93f37c55ab0537cf4d`;
summary Evidence is
`ev_dcbd74c7d4fa42cf6fd74f669cc7e4895cefce86440b002cea1abf1f5a7ea402`.
The complete exported bundle was validated. Its 1,629 semantic unknowns remain;
this focused syntax analysis does not establish whole-application behavior or
runtime reachability. The two changed embedded bindings were separately
checked with Acorn, outside those 20 projections.

REA found an unpacked `node_modules/keytar/build/Release/keytar.node`
contradiction: ASAR declared
`6c32c41c0e5a9e546616607b0383eada5bb646d0164324f20baab59d5b7963fa`,
while actual bytes hash to
`175661065439d76b4e8a522ea378b7bf7760187d7fe94e333e797df748005585`.
That member remains observed-untrusted. The resource observer binds only the
packed JavaScript and Droid bytes and never loads keytar; it makes no claim
about addon execution/integrity, Keychain behavior, native activation or login.
The valid bundle signature is a separate observation, not a substitute for
resolving this mismatch.

Nine isolated regression cases were written before implementation and retained
under fast mode without running the test suite. Their scope includes package
identity, passive discovery without a CLI, selected-copy precedence, symlinks,
Desktop/CLI directory separation, host identity and confined watchers.
