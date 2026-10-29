# Grok Build evidence and passive version identity

Arca reads a version label for the selected official Grok Build **1.0.46**
macOS arm64 image without starting it. The shared Core artifact reader verifies
all 150,374,256 bytes against SHA-256
`e8daa302364c9c3b6a5546d511cfbd1ab5e5d407a9b04282f660665ea405f9f3`.
It retains the selected alias, confined physical file and descriptor identity
through the complete bounded digest read. An unknown image or wrapper returns
unknown; it does not fall back to running `grok --version` or borrowing a version
from a sibling download, filename, README or mutable marker.

This uses the existing artifact-runtime mechanism and its separate 512 MiB
verification allowance. It does not change launch detection, launcher arguments,
MCP/Model writers, user Skills or the native plugin inventory. Custom launch
commands retain the shared launcher's existing unknown-version behavior.

## Distribution binding

The [official installer](https://x.ai/cli/install.sh) publishes the versioned
binary URL pattern and a public Google Cloud Storage fallback. Its fixed
[1.0.46 arm64 object](https://storage.googleapis.com/grok-build-public-artifacts/cli/grok-1.0.46-macos-aarch64)
returned HTTP 200 to an unauthenticated HEAD request. Its stored size and MD5
match the installed image (object generation `1790806024903396`). The remote
metadata did not publish a SHA-256; the complete SHA-256 above was computed
locally. The installer was inspected as source and never executed.

The installed image's strict code-signature verification succeeded. REA observed
Developer ID `X.AI Corporation (5Y6N3AJ54S)`, identifier `xai-grok-pager` and the
hardened-runtime flag. This does not claim target execution or a live account
state, and the artifact label does not establish runtime plugin acceptance.

## Plugin mechanism remains partially verified

The shipped plugin guide documents user plugin directories, Claude-compatible
sources, native manifests and Skill/command/agent/hook/MCP/LSP contributions.
The complete retained README, plugin guide and configuration reference match
bytes embedded in this exact image at file offsets 124067389, 122593134 and
122971276 respectively. This binds the reference documents to the inspected
image; their contents do not by themselves prove native parser semantics.
The current [public plugin documentation](https://docs.x.ai/build/features/skills-plugins-marketplaces)
also describes these capabilities. Current web documentation and the installed
1.0.46 reference are different version surfaces; their path lists are not merged
into an invented current loader contract.

Native install-registry serialization, install-directory authority, full plugin
ID hashing, manifest override precedence and persisted trust resolution still
need stronger native/source evidence. There is no Grok native-plugin observer or
Agent Plugins conformance claim in this change. A marketplace cache is not an
installed instance; enabled metadata does not prove execution or MCP connection.

REA Mach-O evidence is
`ev_708ab8709854a32f757a54231756e91d773ea674bceae3eb9db485e028ef6fc9`;
signature evidence is
`ev_c73244c8bea96de9a4b525f0e184a5ebe47a67777c4395de2703047bc96a0764`.
The complete two-record export is retained in local research. Mach-O enumeration
contained two exported symbols and 480 imports, with incomplete coverage. Deep
native providers were unavailable, so metadata and string observations do not
resolve the outstanding serializer and policy rules.

No target CLI, loader, bootstrap, plugin or hook was executed. Private account
and configuration files, credentials, session databases and Skill bodies were
not read. An isolated
regression source guards the no-execution fallback and is retained unrun in fast
mode; shared artifact tests already cover whole-file hashing, alias CAS and
finite budgets.
