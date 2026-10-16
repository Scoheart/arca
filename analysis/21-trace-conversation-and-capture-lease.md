# Conversation reader and capture lease lifecycle

## Problem and decisions

The previous three-pane Trace view gave conversation text only a 600-character summary column and required selecting every event to read the full text. Empty inspection space dominated the window. Core already owns adapters, native-ID pairing, revision checks and redaction, so the desktop should present a chronological conversation without recreating those rules.

The new two-pane reader retains a compact session sidebar. User messages align right, Agent messages left. Markdown is rendered locally without HTML or image fetches; tools expand inline. Raw JSON, metadata, copying and export remain available through the shared dialog. Adjacent exact-ID tool returns collapse visually; interleaved or unmatched returns stay in sequence. No adjacency-based inference is added to Core.

Core adds redacted inline text with a 32 KiB/message and 512 KiB/page budget. Redaction precedes UTF-8-safe slicing. Tool bodies remain on demand. Frontend inline retention is capped at 2 MiB estimated string memory, and detail cache remains six entries / 4 MiB. Full text uses navigable bounded segments. Nothing executes or uploads log content.

## Capture failure

The screenshot reports an occupied capture lock following an earlier failed session. This does not itself establish that another Arca instance is recording. The code contained a concrete leak: `Child::try_wait` reaped an exited engine leader, then `terminate` returned immediately because that leader had ended. A bootloader/Python helper could retain the inherited flock descriptor. Closing only the parent's file did not release the shared open-file-description lock.

On Linux/macOS, liveness now uses `waitid(WNOWAIT)`, reserving the owned child PID until cleanup. Cleanup signals only the dedicated engine process group, reaps its leader, and explicitly unlocks the lease. Keeping the leader unreaped prevents PID reuse between inspection and signaling. The inherited lease still protects a live engine when Arca crashes. A start guard rejects overlapping initialization within the same Arca process. A busy lock stays protected; no lock-file unlinking, broad process killing or Agent termination is used.

Existing old-version orphan processes on a user's Mac cannot be inspected from this Linux environment. The busy notification no longer asserts an unverified second instance and explains stopping existing recording or exiting the old version before retrying.

## Verification

- Production Core/CLI and desktop compilation.
- Isolated actual CLI reads of Pi JSONL and Gemini documents: Unicode, inline budgets, continuation without duplicated IDs, full paired output, secret redaction before slicing, and source immutability.
- Actual Worker source exercised on Linux: exited shell leader with live helper holding its inherited lease; cleanup released the lock and killed the owned helper while preserving an unrelated process group.
- Chromium component review with explicitly synthetic data at 1200×820, 900×600 and 600×700: message alignment, Markdown, tool expansion, original-record dialog, Escape, and sidebar navigation; no JavaScript errors, remote image fetches or horizontal page overflow. This is not native macOS acceptance.
- Regression fixtures added for inline budgets/redaction, exact-ID folding, retained-body budgets, and inherited capture locks. Test suites are not run under the repository's fast-delivery policy.
- Native `tauri dev` attempted with actual HOME; blocked by missing Linux GObject development libraries. macOS app packaging/launch checks remain in release CI; no local Mac installation claim is made.
