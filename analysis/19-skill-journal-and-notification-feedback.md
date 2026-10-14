# Skill journal retirement and operation feedback

Date: 2026-10-14. Based on main at `8ddffc2` (v2.0.9).

## Recovery failure

Successful Skill transactions retired their verified journal, then attempted to
delete the shared empty journal directory. A pre-existing sibling cleanup
quarantine could make that final directory deletion fail. This converted a
completed operation into a rollback and then `recovery_required`, even when no
active journal remained. Registration and assignment became unavailable.

Retirement now removes only the operation's verified journal records and keeps
the shared directory stable. It does not remove unrelated cleanup residues.
Startup recovery, strict journal decoding, ownership checks, CAS and rollback
protection remain in Core. A fresh successful inventory also clears an older
Desktop recovery latch.

## Common feedback

Operation results, validation, recovery, startup synchronization and updater
feedback all use `ToastProvider` / `ToastNotice`. One body portal sits at the
top right above dialogs and enters horizontally from the right. Repeated
messages are deduplicated. Stable keys update background progress in place;
operation results can interrupt it. Resolving a source withdraws its notice.
Notifications preserve editor focus, support dismissal and reduced motion,
and retain retry, update and restart actions. Resource status and inspection
evidence remain ordinary data.

## Verification

- Reproduced the v2.0.9 failure with the real CLI and an isolated HOME containing
  a harmless public Skill and an unrelated cleanup sentinel.
- The fixed CLI imports, unassigns, assigns and removes that Skill successfully
  with the same residue. The shared journal directory stays empty; unrelated
  sentinel and Agent settings remain unchanged.
- Valid active, retiring and retired journals recover on startup. Malformed
  journal evidence remains intact and keeps Skill mutations unavailable.
- React and the actual dialog/notification components verified portal placement,
  deduplication, progress updates, interruption, dismissal and focus behavior.
- CLI compilation, Desktop TypeScript/Vite production compilation and website
  compilation passed. Test suites were not run under the project fast-mode rule;
  regression fixtures were updated for an explicitly requested future run.
- Native `tauri dev` was attempted with the real HOME. This Linux host lacks
  GLib/GObject desktop dependencies and cannot provide macOS GUI acceptance or
  install `/Applications/Arca.app`; the macOS release build performs packaging
  and signing checks separately.
