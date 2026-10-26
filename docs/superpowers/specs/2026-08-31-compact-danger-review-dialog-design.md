# Compact Danger Review Dialog Design

## Goal

Make destructive confirmations feel deliberate and polished without turning them into large review workflows. The immediate target is **Clear all Models** in an Agent workspace; the shared treatment also improves the small **Delete source** confirmation.

## Chosen direction

Use a compact macOS-style danger card inside the existing portal-backed modal system.

- Keep the current `Modal` and `DialogShell` focus trap, Escape handling, inert background, busy state, and error reporting.
- Narrow danger reviews to the existing small dialog size and remove the oversized gray footer tray.
- Add a restrained red-tinted trash glyph before the heading so the action reads as destructive before the user reaches the button.
- Use a solid red confirmation button only inside danger review dialogs. Existing inline `.btn-danger` actions remain quiet text buttons.
- Keep cancellation visually neutral and place both actions directly on the dialog surface.

## Clear Models content

The dialog uses this hierarchy:

1. Title: `清空全部 Models`
2. Subtitle: `<Agent name> · <count> 个 Model`
3. Primary impact: all Model entries in the current Agent configuration will be removed.
4. Danger note: external and manually configured entries are included, and the action cannot be undone from the dialog.
5. Preservation note: central Models, Providers, and credentials remain intact.
6. Confirmation label: `清空 <count> 个 Models`

The copy avoids filenames and internal plan details. It describes only the user-visible scope.

## Alternatives considered

### Restyle every modal

Rejected because editor and picker dialogs need different density and footer behavior. A global change would create unnecessary visual regressions.

### Inline confirmation beside the toolbar button

Rejected because the destructive scope includes external configuration. A modal interruption is appropriate and preserves keyboard/focus accessibility.

### Dedicated one-off Clear Models component

Rejected because Delete Source needs the same small danger treatment. Extending the existing `ReviewDialog` keeps behavior consistent without duplicating modal infrastructure.

## Component changes

- `DialogShell` receives an optional leading visual and optional class name.
- `ReviewDialog` supplies the danger glyph, danger-specific shell class, and solid confirmation button.
- `AgentView` supplies structured Clear Models impact copy and the count-aware label.
- `index.css` owns the danger review visual treatment and responsive compactness.

## Accessibility and behavior

- The title remains the dialog accessible name.
- Initial focus remains on the title; the glyph is decorative.
- Escape, scrim close, opener focus restoration, busy locking, and error alerts are unchanged.
- Red is not the only signal: icon, title, explicit scope, and button wording all communicate danger.

## Verification contract

- Clear Models renders the compact danger shell, glyph, count-aware subtitle and action label.
- The dialog states both destructive scope and preserved central data.
- Confirm, cancel, busy, focus, and error behavior remain unchanged.
- Delete Source continues to work and inherits the shared danger presentation.
- Local tests/build/formatters remain skipped under Arca fast mode; regression tests are retained and the production Stable build is required.
