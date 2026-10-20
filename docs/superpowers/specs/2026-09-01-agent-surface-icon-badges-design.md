# Agent Surface Icon Badges Design

> Historical design proposal. The current five-surface taxonomy, component structure and size rules are documented in [Agent 形态角标](../../agent-surface-badges.md).

**Date:** 2026-09-01  
**Status:** Approved visual direction; pending implementation  
**Visual direction:** B — device-symbol corner badges

## Goal

Make Agents that intentionally share a brand logo distinguishable at icon-only sizes. The first required case is Claude Code versus Claude Desktop; Qoder CLI versus Qoder Desktop follows the same rule.

The base brand logo remains unchanged. Arca adds a small lower-right device symbol only when a shared logo represents multiple product surfaces.

## Existing Context

- `desktop/src/components/brandIcons.tsx` is the single renderer used by the pinned Agent bar, Agent picker, Agent detail header, add dialog, and resource-consumer stacks.
- `desktop/src/assets/agents/aliases.json` maps both `claude-code` and `claude-desktop` to `claude.svg`, and both Qoder variants to the Qoder asset.
- `AgentGlyph` currently renders at 20, 24, 30, 32, 42, and 44 pixels.
- Existing Agent `category` values describe product/catalog grouping, not a reliable execution surface. For example, `coding-agent` includes pure CLI tools and mixed products. Badge selection must not infer surface from `category`.

## Scope

### Included

- Four surface values: `cli`, `desktop`, `ide`, and `web`.
- Explicit surface metadata for built-in Agents that need icon disambiguation.
- Automatic collision detection using the resolved logo asset key.
- A reusable device-symbol badge rendered by `AgentGlyph` in every existing context.
- Light and dark theme styling for all supported glyph sizes.
- Focused component, navigation, and CSS contract tests.

### Excluded

- Replacing or editing vendor logos.
- Adding text such as `CLI`, `APP`, or `DESK` inside the badge.
- Guessing a surface for custom or unknown Agents.
- Showing a badge on every Agent regardless of logo collision.
- Changing Agent capability, configuration, installation, or persistence behavior.

## Data Model

Add `desktop/src/assets/agents/surfaces.json` as presentation metadata:

```json
{
  "claude-code": "cli",
  "claude-desktop": "desktop",
  "qoder-cli": "cli",
  "qoder": "ide"
}
```

The schema accepts only `cli`, `desktop`, `ide`, or `web`. It is independent of `aliases.json`:

- `aliases.json` answers which brand asset to render.
- `surfaces.json` answers which product surface a variant represents.

The initial rollout contains only unambiguous variants required for shared-logo disambiguation. Additional built-ins can be annotated later without changing the renderer. Custom Agents and unknown IDs remain unbadged.

At module initialization, `brandIcons.tsx` resolves each annotated Agent to its final logo key using `ICON_ALIASES[id] ?? id`. A logo key is considered a collision group only when at least two annotated Agent IDs resolve to that key and the group contains at least two distinct surfaces.

An Agent receives a badge when all conditions are true:

1. it has explicit surface metadata;
2. it has a real resolved logo asset;
3. its logo belongs to a collision group with more than one surface.

This policy produces badges for both members of the Claude and Qoder pairs while keeping unrelated icons visually clean.

## Visual System

| Surface | Symbol | Color |
|---|---|---|
| CLI | terminal prompt | graphite |
| Desktop | monitor | Arca blue |
| IDE | split editor pane | violet |
| Web | browser/globe | teal |

Symbols are compact inline SVGs with no text. Badge size is derived from the base icon size:

| Base glyph | Badge |
|---|---|
| 20–24 px | 10 px |
| 25–36 px | 12 px |
| 37 px and above | 14 px |

The badge is anchored to the lower-right corner and offset outward by two pixels. A two-pixel theme-aware separation ring keeps the badge legible over light and dark surfaces. The badge must not change the base glyph's requested width or height.

## Component Structure

`AgentGlyph` becomes a consistently positioned wrapper for every rendering branch:

```text
AgentGlyph wrapper (requested width and height, overflow visible)
├── base tile (owns radius and clipping)
│   └── logo image or monogram
└── AgentSurfaceBadge (optional, absolute lower-right)
```

Today, full-bleed images return a bare `<img>` while mark-only logos and monograms return a tile. The implementation will normalize those branches behind the wrapper without changing their current visual treatment:

- full-bleed app icons still cover the complete base tile;
- mark-only logos still sit on the existing white/themed tile;
- wide tiles keep their current background and containment;
- monogram fallback remains unchanged and never receives an inferred badge.

`AgentSurfaceBadge` remains private to `brandIcons.tsx` unless another component develops an independent need for it. Page components must not contain Claude-, Qoder-, or surface-specific conditionals.

## Accessibility

- The full Agent name remains the accessible label through the existing image alt text, button label, or surrounding title.
- The decorative badge is `aria-hidden="true"`.
- Surface is not communicated by color alone: every surface has a distinct silhouette.
- The badge must maintain recognizable contrast in light and dark themes.

## Failure and Fallback Behavior

- Missing, unknown, or invalid surface metadata results in no badge.
- A surface annotation without a real logo does not decorate the monogram fallback.
- A logo used by only one surface does not receive a badge.
- No metadata condition may prevent the base Agent icon from rendering.

## Verification

Focused automated coverage will verify:

1. Claude Code renders the CLI badge and Claude Desktop renders the Desktop badge.
2. Qoder CLI renders the CLI badge and Qoder Desktop renders the IDE badge.
3. A unique-logo Agent renders no badge.
4. An unknown or custom Agent renders its existing fallback without a badge.
5. The badge is decorative and the existing accessible Agent name is preserved.
6. Badge sizing follows the 20/24, 30/32, and 42/44 pixel tiers.
7. Pinned navigation and Agent picker rendering continue to use the shared component.

Visual QA will compare 20, 24, 30, and 44 pixel glyphs in both themes. The badge must remain identifiable without obscuring the Claude or Qoder brand mark.

## Acceptance Criteria

- Claude Code and Claude Desktop are immediately distinguishable in the icon-only pinned bar.
- The same distinction appears everywhere else `AgentGlyph` is used.
- Qoder CLI and Qoder Desktop follow the same automatic rule.
- Agents without shared-logo surface conflicts remain visually unchanged.
- No page contains hard-coded special cases for individual Agent IDs.
- Existing icon sizing, alignment, alt text, and full-bleed behavior remain intact.
