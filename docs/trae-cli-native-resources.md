# TRAE CLI native resources and passive release identity

Arca can label one exact official TRAE CLI macOS arm64 main image as release **0.212.1** without executing it. This identifies the selected release artifact. It does not observe CLI stdout, establish plugin enablement or verify a complete working installation.

TRAE CLI is distinct from the desktop TRAE products and the separately cataloged open source [TRAE Agent](https://github.com/bytedance/TRAE-agent). Official documentation calls the current CLI generation **TRAE CLI 2.0**; the audited publisher version is **0.212.1**. The generation label must not replace the artifact version.

## Official artifact and provenance

The [official upgrade documentation](https://docs.trae.cn/cli_get-started-with-trae-cli?lang=zh) links the public [install_v2.sh](https://trae.cn/trae-cli/install_v2.sh). The script was downloaded and read as public source, never run. The observed 90,907-byte script has SHA-256:

```text
9f50968efa9ea67c9840bfbeb318d3378e011084ab1511a5d0e514fa0bd33ff1
```

Its default public/stable selection uses the [stable latest manifest](https://lf-cdn.trae.com.cn/obj/trae-com-cn/trae-cli/v2/public/stable/latest.json). At acquisition on 2026-10-29, latest was byte-identical to the [fixed 0.212.1 manifest](https://lf-cdn.trae.com.cn/obj/trae-com-cn/trae-cli/v2/releases/0.212.1/manifest.json), both 5,110 bytes with SHA-256 `006fccd0ba8873bdb98b2e4ff903a81df8034d8a2a9a45c3b17d087f970d8557`.

The manifest declares product `traecli`, distribution/channel `public`, release channel `stable`, and matching `version`/`binary_version` values of `0.212.1`. Its release timestamp is `2026-10-29T05:03:42.402098Z`. Fields `build_channel` and `binary_source` are `internal`; they are publisher metadata within the public release. The declared source commit `6af407b70470c550c9a310f9678f738cb33182d7` and source-manifest digest were not independently checked against a retrieved source tree.

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| [Official main gzip](https://lf-cdn.trae.com.cn/obj/trae-com-cn/trae-cli/v2/releases/0.212.1/traex-macos-aarch64.gz) | 142,894,287 | `396052a5f0de907254fa490251f5be7d0b2a7d9707337d41639a585f824f3119` |
| [Official code-mode-host gzip](https://lf-cdn.trae.com.cn/obj/trae-com-cn/trae-cli/v2/releases/0.212.1/traex-code-mode-host-macos-aarch64.gz) | 52,104,844 | `c376c38b1e565aea75f646f0986e79f744a44115766187e693784802cba43ed4` |
| Original decompressed main `traex` | 362,201,088 | `46d6b93e1d642a0df9fa621b4fbfe9f62ffec57b921aa8637c27390cd70a3c40` |
| Original decompressed `traex-code-mode-host` | 269,757,696 | `5c8950ab8f3b51d938582414e7ab92605b755c1a7b014debc1e5faca3f89252a` |

Both compressed artifacts matched the manifest's complete size and digest. System gzip decompression produced two arm64 Mach-O executables. Their original embedded linker ad-hoc signatures passed strict integrity verification; neither has a Team Identifier. This is not Developer ID publisher authentication, an independently verified release signature or a verified runtime dependency closure. No bytes were re-signed.

The descriptor in [native-runtime-trae-cli.json](../data/native-runtime-trae-cli.json) is the source of truth for Arca's exact main-image identity. Its three fields are parsed through the shared strict Core contract; unknown fields cause a passive miss.

## Passive version boundary

[trae_cli.rs](../core/src/resources/plugin/trae_cli.rs) delegates to the shared `artifact_runtime` Core mechanism. That mechanism resolves only the explicitly selected path, verifies a confined regular file with its complete byte length and SHA-256, holds the opened descriptor, then rechecks both selected-path and physical-file identity. Explicit aliases are allowed only while their selection remains stable.

Each version lookup receives a fresh finite **512 MiB** digest allowance, separate from the private resource-metadata read allowance. The 362,201,088-byte main image fits within that bound. Unknown, truncated, enlarged, changed or substituted images return no version. A passive miss remains handled, so it cannot trigger an automatic CLI `--version` fallback.

A full match may be displayed as **official release artifact version 0.212.1**, because those bytes trace through the verified gzip digest to a fixed manifest whose `version` and `binary_version` agree. No CLI output was observed. A matching main image does not verify the sidecar, native addons, plugin workflow, login, configuration or effective capabilities.

The companion's identity is recorded separately above. It is not used to borrow a version for an unknown main image, and its presence is not evidence that it is an MCP server. Mutable `current_version`, release directory names, `trae-package.json` and `trae-installation.json` cannot replace main-image identity.

## Official installation and launcher layout

The audited public installer defines these paths:

| Setting | Source-defined default or precedence |
|---|---|
| Visible commands | `TRAECLI_INSTALL_DIR`, then `XDG_BIN_HOME`, then `~/.local/bin` |
| Selection state | `TRAECLI_STATE_DIR`, otherwise `${XDG_DATA_HOME:-~/.local/share}/traecli` |
| Releases | `TRAEX_RELEASES_DIR`, otherwise `STATE_DIR/releases` |
| Current selection | `STATE_DIR/current` |
| Main file | `RELEASES_DIR/0.212.1-aarch64-apple-darwin/traex` |
| Companion | `traex-code-mode-host` beside the main file |

Its default chain is:

```text
~/.local/bin/traecli
  -> traex
  -> ~/.local/share/traecli/current/traex
  -> releases/0.212.1-aarch64-apple-darwin/traex
```

These are logical relative-link relationships; the installer calculates relative targets for the configured directories. It also offers `coco`, `trae-cli`, `trae-agent` and `ta` compatibility names. Arca's default TRAE CLI launcher selects **traecli**. It does not add `trae-agent` as a fallback, because that name also belongs to the other product. Detection of an executable can report installation availability while its passive artifact version remains unknown.

A daemon-package installer mode additionally creates `bin/traex`, `bin/traex-code-mode-host` and `trae-package.json` with `layoutVersion=1` and entrypoint `bin/traex`. This mode and the installer's comment that runtime policy is read beside the executable are public source observations, not independently proven runtime reads.

The installer source data/control flow was traced through variable initialization (lines 74–98), `detect_os`/`detect_arch` (272/280), `resolve_manifest` (1720), `resolve_latest_manifests` (1905), platform-target mapping (2020), hash/decompression (2061–2077), staging (2105) and visible/current publication (2157–2203). `resolve_manifest` restricts the selected platform, channel, distribution, version, download prefix, compression and declared size/SHA.

## Static staging and execution boundaries

Retrieving public manifests and gzip files uses the network. Once complete official bytes are acquired, size/SHA checks, gzip decompression, ordinary file copying, permissions and relative-link construction can be performed as separate static operations. They do not require executing TRAE CLI.

The public installer combines those operations with execution and user-state mutation. It must not be invoked as a passive probe:

- `release_dir_is_complete` (1047) runs the candidate `--version` at 1081.
- Main smoke checks occur at 2099, 2113, 2161 and 2222; line 2102 starts the sidecar.
- `prepare_executable` (2086) can clear xattrs and apply an ad-hoc signature.
- `install_dependencies` (811) manages ripgrep and Linux bubblewrap; `check_node_dependency` (827) checks Node/npx. Dependency acquisition can use the network, and Linux probes execute bubblewrap.
- Selection publication writes policy/current/version state and command links; `ensure_path` (920) handles shell profile/PATH setup. Other branches handle legacy commands, Brew migration and release cleanup.

Acquisition preserves non-executable original bytes in an isolated research prefix. The reviewed main program and companion were then copied into a new user-level release directory with executable permissions. Only the primary `traecli` alias was added, directly selecting that release; existing native current-selection, policy, compatibility aliases and shell profiles were untouched. No installer, CLI, `--help`, `--version`, login, import, bootstrap, sidecar, plugin, hook or helper was executed. Installation receipts distinguish this static file installation from unobserved runtime behavior and dependency closure.

[Official migration documentation](https://docs.trae.cn/cli_get-started-with-trae-cli?lang=zh) says first 2.0 startup can detect legacy user/project resources and sessions, offer imports, and write user/project `traecli.toml`. Its examples include external agent data. Those claims reinforce why target startup was excluded; no actual private configuration or resource body was inspected.

## Documented resources and unverified loader

The [official extensions page](https://docs.trae.cn/cli_tools-and-extensions) positively describes plugins grouping commands, Skills and connectors, Skills supplying domain instructions, and MCP connecting external tools/services. It documents `/plugins`, `/skills` and `/mcp`. The [slash-command reference](https://docs.trae.cn/cli_slash-command) also lists `/agents` and `/hooks`; the [CLI reference](https://docs.trae.cn/cli_command-line-parameters?lang=zh) lists a `plugin` subcommand.

These are official documentation claims, not a verified runtime main → config → resource-reader chain. Exported native symbols include plugin/skill modules, format-adapter names and an `agent_plugin_schema_status` locator. Names and addresses alone do not establish the accepted manifest, runtime branch, discovery root, effective enablement or Agent Plugins v1 compatibility.

No native analysis provider could be bound in the available REA environment: existing diagnostics reported Ghidra unconfigured with an insufficient Java runtime, Hopper unavailable and IDA unconfigured. The task did not retry that shared session or install an engine/runtime. **There is no successful native REA decompilation evidence.** Header/signature/manifest inspection does not fill that gap.

Native plugin roots, manifest/marketplace schema, profile/project precedence, enabled defaults, duplicate handling and effective MCP/Skills/hooks/commands/LSP contributions remain unknown. The selected public release record supplies native gzip artifacts without a source-archive entry; that bounded observation does not prove source is unavailable elsewhere or that no plugin reader exists.

This change creates no native-plugin host or observer. It does not count sidecars, symbol names, connectors or arbitrary plugin files as MCP resources. A future observer requires a proven runtime reader contract before it can expose metadata.

## Existing configuration boundary

[Official configuration documentation](https://docs.trae.cn/cli_config-file) gives `~/.trae/traecli.toml` and a default stdio example:

```toml
[mcp_servers.docs]
command = "node"
args = ["/path/to/server.js"]
```

That supports Arca's existing narrow default-TOML stdio writer description; it does not prove support for arbitrary profiles, old YAML, remote MCP, plugin-contributed MCP or Skills. This task adds no writer and found no evidence that the narrow existing writer is completely invalid.

The [environment reference](https://docs.trae.cn/cli_environment-variables) says `TRAE_HOME` overrides CLI configuration/runtime directories. Installer-only `TRAEX_HOME`, `TRAECLI_HOME` and XDG path derivations must not be promoted into proven runtime configuration resolution. The CLI reference documents `--profile`, repeated dotted `-c` overrides and `exec --ignore-user-config`; effective merge order and project discovery remain unknown.

## Regression source and verification status

Seven isolated `TestHome` regressions were written before this adapter: default launcher/other-product separation and no execution; complete fixture digest with same-size, enlarged and truncated changes; unknown-selection fallback and mutable metadata; sidecar/product identity; independent finite digest budget; alias and replacement CAS; strict descriptor fields.

The small shell fixture has a literal independent size/SHA and writes a marker if executed. Assertions exercise real shared Core verification and launcher behavior, without mocks or a large official-binary fixture in the repository.

Fast delivery retains the regression source without running tests, formatters or validators. Production compilation, actual-user inventory comparison and development-preview evidence are recorded separately. No target probe or passing test suite is claimed.
