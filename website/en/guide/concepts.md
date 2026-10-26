# Core concepts

## Central assets and Agent relationships

Models, MCPs, and Skills each have a central library. An **asset** defines the content; a **consumption relationship** defines which Agent should use it. Agent pages select assets and manage relationships. Create, import, edit, and delete assets in the top-level libraries.

| Action | Relationship | Central asset |
|---|---|---|
| Add to an Agent | Establish use | Kept |
| Disable | Retain use, pause activation | Kept |
| Remove use | Remove that relationship and safely removable managed targets | Kept |
| Delete a central asset | Review all consumers and clean managed relationships | Removed or moved to backup |

An Agent can use several MCPs and Skills. Native multi-model Agents can retain multiple Models with at most one global current Model. Some desktop clients select Models per conversation. The client's verified contract determines the limits.

## MCP sources and precedence

The central catalog is the union of enabled remote subscriptions, imported local configurations, and manual or pasted entries. The optional curated collection is a remote subscription.

Manual entries take precedence when the same `name::transport` occurs in several sources. Supported legacy managed discovered entries still participate. Shadowed copies remain visible with their source. Transport identity is `stdio` or `http`, with SSE grouped under HTTP; same-named stdio and HTTP entries are distinct assets.

New discoveries in Agent files are external observations, not automatically created catalog sources. Import explicitly to create a central asset.

## Desired and observed state

Arca records what an Agent should use and reads files and Skill links to compare that with reality.

| State | Meaning |
|---|---|
| Synced | Content and relationship agree |
| External added | Present in the Agent, not managed by Arca |
| External changed | Managed content was changed elsewhere |
| External removed | Relationship remains, target is missing |
| Unparseable / ambiguous / unsupported | The target cannot be read safely or represented losslessly |

For an exact affected relationship, Arca exposes only safe available actions: **adopt, restore, detach**. Scanning never adopts content automatically. Restoring does not overwrite an external directory, file, or foreign symlink.

## Synchronization and recovery

Central changes persist first. Each `Agent × capability × physical target` then converges independently. Agents sharing a target appear together in the impact review. A failed target does not roll back other completed targets; its relationship remains for targeted repair or retry.

Existing Agent files update in place after backup and concurrency checks, preserving the path and inode for clients watching the file. New files and Arca-private documents publish atomically. A changed target invalidates an old review.

## Data layout

```text
~/.arca/
├── settings.json                  # Preferences and relationships
├── assets/
│   ├── mcps/                      # Catalog, sources, MCP icons
│   ├── models/                    # Provider / Model metadata
│   └── skills/items/              # The central Skill copy
├── staging/                       # Temporary parsing and reviews
├── journals/                      # Operation and recovery progress
└── backups/                       # Pre-change backups
```

Central API keys live in the system Keychain, outside this public metadata. Skill assignments are links to the central copy. Paths inside the user directory use portable `~/…` notation; verify custom paths and runtime environments when moving machines.

[Desktop guide](/en/guide/desktop) · [CLI / TUI](/en/guide/cli)
