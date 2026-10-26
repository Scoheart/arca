# User-level Skills

A Skill is an instruction directory containing `SKILL.md`. Arca keeps one central copy and assigns links into verified user-level Agent directories. It manages global Skills.

## Get a Skill

| Source | Workflow |
|---|---|
| GitHub | Enter a public repository or subdirectory URL and press Enter or click Import |
| Local folder | Select one Skill or a directory containing several |
| Archive | Import `.zip`, `.tar.gz`, `.tgz`, or `.tar` |
| External Agent copy | Observe it first, then explicitly import that exact copy |

GitHub, folder, and archive installation does not require Git, Node.js, or `npx`. Download or import creates a central asset; assigning it is a separate step.

Submitting a source directly imports every discovered Skill without a second installation review. Use a Skill's subdirectory URL to import only that Skill. Same-name central copies are backed up before replacement; existing assignments and disabled states are retained. Core still validates paths, archives, and candidate content. Importing does not execute scripts.

## Source navigation and cards


The left sidebar shows concrete sources. Subdirectories of one GitHub repository group together; same-named folders in different locations remain separate. Search applies to the current scope.

Cards show a name, up to three description lines, and readable-target Agent icons. Icons form a small hand of cards that expands on hover or keyboard focus; clicking opens the Agent's Skills page. Details distinguish central content and actual copies. External copies do not inherit the central source or risk assessment.

## Assign, disable, and remove use

Select a central Skill in the Agent's Skills tab. Its user-level link points to:

```text
~/.arca/assets/skills/items/<skill-name>/
```

Several Agents can read one physical directory; reviews list everyone affected. Disabling keeps the relationship and central content. Removing use keeps the central asset and removes only a provably managed link.

External directories, files, and foreign links remain intact. When several physical targets exist, CLI enable/disable uses `--target <target-id>` to choose precisely.

## Updates and local changes

Checking updates reads version or content state without changing the Skill. Ordinary low-risk updates execute directly; overwriting local edits, conflicts, or high-risk candidates requires one review.

Reinstalling, importing the same central name, and updating preserve existing consumers and disabled state. An upstream revision change with identical content updates version metadata only. Adoption rereads and audits the current content, binding its hash; edits after review invalidate the old plan.

A managed link exposes the central copy, so editing through it changes central content too. Arca detects that change rather than treating it as an independent external copy.

## Risk and recovery

Arca checks paths, links, archives, structure, and content hashes without running candidate scripts. `SKILL.md` renders as text, without executing embedded HTML or remote resources.

High-risk approval binds the exact content hash. Assigning or enabling that approved central version does not repeat the same findings confirmation; changed content is audited again. Authority-reducing actions are not blocked by the risk gate.

Central state persists first and each physical target converges independently. Failed targets retain a pending relationship while successful targets remain complete. Details expose repair. Central deletion reviews consumers and moves content to timestamped backup.

## Current limits

Project-level Skill writes, authenticated private Git sources, and creating or editing `SKILL.md` inside Arca are not supported. See [Supported Agents](/en/guide/agents#skills) for directory contracts.

[CLI Skill commands](/en/guide/cli#skill) · [Watch the demo](/en/guide/demo)
