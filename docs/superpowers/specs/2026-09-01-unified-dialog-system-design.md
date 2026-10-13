# Unified Dialog System Design

## Summary

Arca will replace its current collection of visually divergent dialogs with one compact desktop dialog system. The approved direction is flat, icon-led, and information-complete: necessary values remain visible, icons replace repeated labels and obvious actions, and content is organized with spacing and quiet dividers instead of nested cards.

This change covers resource details, editors, pickers, reviews, confirmations, and multi-step Skill dialogs. It changes presentation only; resource behavior, persistence, validation, and modal accessibility semantics remain unchanged.

## Problems to solve

- Resource inspectors use a fixed 680 px inner height, producing large empty areas for short content.
- Headers, bodies, close buttons, and footers vary between the generic shell, resource inspectors, Provider dialogs, Agent dialogs, and Skill dialogs.
- Feature-specific `:has(...)` overrides create multiple unrelated spacing systems.
- Details are presented as cards inside a card-like dialog, making the hierarchy noisy.
- Repeated labels and explanatory copy slow scanning even when an icon or short value is sufficient.
- Action areas are visually heavy and sometimes occupy more space than the content.

## Design direction

The visual tone is a restrained macOS utility panel: opaque surfaces, precise spacing, short motion, and a single blue action accent. The dialog itself is the only elevated surface.

Rules:

1. A dialog body is one continuous content plane. Do not place ordinary sections inside additional filled cards.
2. Use spacing and one-pixel horizontal dividers to express hierarchy.
3. Keep necessary information visible. Do not hide required identifiers, URLs, impact, or validation state merely to make a dialog shorter.
4. Use icons for common metrics, copy, close, edit, delete, status, protocol, and capability. Keep a short text label when an icon alone is ambiguous or destructive.
5. Give every icon-only action an accessible name and Tooltip.
6. Let content determine dialog height until the viewport limit is reached. Only the body scrolls.

## Shared structure

`Modal` remains the portal, stack, focus-trap, inert-background, Escape, and scrim authority.

`DialogShell` remains the shared semantic shell for editor, picker, and review dialogs. Its header, body, status, and footer become the visual source of truth. Feature dialogs may choose size and content layout, but may not restyle shell geometry through descendant-specific `:has(...)` rules.

`ResourceInspector` remains inside the `Modal` owned by `ResourceWorkspace`, but adopts the same header, body, footer, close-control, divider, and spacing rules as `DialogShell`. Its fixed inner height is removed.

The four shell sizes remain semantic rather than feature-specific:

| Size | Target width | Typical use |
|---|---:|---|
| `sm` | 400–440 px | Confirmations and focused settings |
| `md` | 520–560 px | Simple editors and compact pickers |
| `wide` | 620–660 px | Provider and Agent editors |
| `lg` | 700–760 px | Resource details and multi-step review |

All sizes keep a 16 px viewport margin and a viewport-bounded maximum height. Headers are approximately 56 px; footers are approximately 52 px. Neither grows to fill unused space.

## Dialog archetypes

### Resource details

- Header: resource avatar, title, one compact subtitle line, close icon.
- First row: important metrics such as context, output limit, capability, status, or price. Each metric uses one icon, one value, and at most one short label.
- Body: optional one-paragraph description followed by a flat, responsive two-column field grid.
- Long identifiers and URLs remain visible, truncate safely, and provide copy actions.
- Footer: destructive action at the start; copy/more and primary edit actions at the end.
- Short details produce a short dialog. Sparse content must never leave a large blank center.

### Editors

- Header: action icon, title, compact resource context, close icon.
- Fields use a direct one- or two-column grid. Grouping is expressed with gaps and dividers, not nested section panels.
- Protocols, credential modes, and similar peers may use compact segmented controls or flat rows.
- Validation appears directly below its field or in the shared status slot.
- Footer: Cancel and one primary action. No explanatory footer panel.

### Pickers

- Header and footer use the shared shell.
- Search is the first body control.
- Results are flat rows separated by spacing or hairlines; selected state uses a quiet accent surface.
- Count and selected summary stay concise. Empty states do not create another card.

### Reviews and confirmations

- Width stays compact.
- One semantic icon communicates danger, warning, or success.
- Title states the decision; one short sentence states impact and preserved data.
- Do not expose internal execution plans, technical write sets, or redundant summaries unless an error requires action.
- Destructive actions retain a short text label. Icon-only destructive confirmation is not allowed.

### Skill workflows

- Continue using `DialogShell` and the established modal stack.
- Remove the parallel Skill-specific header/body/footer chrome.
- Steps may retain their functional content, but ordinary sections become flat groups.
- Risk details remain explicit; visual simplification must not hide security decisions.

## Icon and copy policy

Icon-only controls are appropriate for close, copy, reveal/hide, refresh, more, and repeated row actions. Edit and destructive actions use icon plus a short label when they are the primary decision in a footer.

Metric icons may replace field labels only when the adjacent value and Tooltip make the meaning unambiguous. Provider, model ID, request URL, environment variable, path, and destructive impact retain visible short labels.

Explanatory copy is removed when it repeats the title, visible field, or action. Error messages, irreversible consequences, and preserved-data statements remain visible.

## Styling migration

- Introduce shared dialog geometry and spacing tokens in `desktop/src/index.css`.
- Group `DialogShell` and `ResourceInspector` structural selectors so they cannot drift.
- Remove fixed inspector height and filled inspector body/footer containers.
- Replace Provider catalog, Provider form, and Agent form `:has(...)` shell overrides with explicit shell variants or content classes.
- Align Skill install/review/risk dialogs with the shared shell and remove their duplicate chrome.
- Keep product color tokens, dark mode, focus styles, and reduced-motion behavior.

## Responsive behavior

- At `1200 × 820`, details use the two-column field grid and editors use their intended width.
- At the product minimum `900 × 600`, dialogs retain 16 px outer margins, bodies scroll, and headers and footers remain visible.
- Below the internal content breakpoint, metric rows wrap and field grids become one column.
- Long names, paths, IDs, and URLs truncate or wrap within their own row; no horizontal overflow is allowed.

## Accessibility

- Preserve modal focus trapping, focus restoration, `aria-modal`, inert background, and topmost Escape handling.
- Every icon-only control has an accessible label and Tooltip.
- Visible focus indicators remain on all controls.
- Destructive and validation text never relies on color or icon alone.
- Dynamic status remains announced through existing status and alert semantics.

## Verification

- Add structural tests for the shared shell, content-driven inspector height, accessible icon-only controls, and the absence of feature-specific shell `:has(...)` overrides.
- Keep existing modal stack, focus, Escape, and responsive overflow tests.
- Build the desktop production bundle through Direct Stable.
- Verify the official Stable assets, signature, DMG, updater, and CLI.
- Final visual acceptance uses only the official `/Applications/Arca.app`; if local replacement is not authorized, report asset verification and leave installation to the user.

## Non-goals

- No changes to model, MCP, Skill, Agent, or credential behavior.
- No new animation framework, font family, or icon library.
- No redesign of the underlying workspace pages, navigation, or resource cards.
- No removal of necessary security, validation, or destructive-impact information.
