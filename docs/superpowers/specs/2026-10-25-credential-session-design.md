# Arca application credential session

## Approved behavior

One application-level Keychain unlock covers all Arca-managed Provider credentials during this desktop process. The session ends on process exit or after a maximum of eight hours (implementation default). Do not request another per-Provider authorization during ordinary reads. Initial import of old independent Keychain items may require separate system confirmations, as explained and accepted in the conversation. System-wide certificate trust and unrelated applications' credentials are outside this session.

## Approach

Use one Keychain-only authorization vault, `com.scoheart.arca.credential-session.v1`, account `api-key`, labelled `Arca · Credentials`. Its versioned payload contains the Arca-owned credential service, exact source-item metadata revision and secret bytes for each entry. The original independent items remain the authoritative compatibility projection for existing Agent `/usr/bin/security` commands. This avoids rewriting Agent files or invalidating running clients merely to change the authorization experience. Provider logical ownership remains separate inside the shared vault.

A successful startup unlock loads the entire vault into zeroizing process-local memory. Reads validate the corresponding source item's metadata without authentication UI and reuse that entry. Entry revisions prevent a stale aggregate from overriding external replacement/deletion. New or changed credentials refresh the in-memory session and best-effort persist the aggregate after the authoritative write. Vault persistence is a derived optimization: failure must never roll back, erase or hide a successfully saved source credential. No credential bytes enter files, configuration, argv, logs or diagnostics.

The alternative of extending the existing per-Provider cache was rejected because each Provider would still require its own first authorization. Destructive removal of old Keychain entries was rejected because existing native Agent helpers still reference them.

## Lifecycle and concurrency

Desktop warms the session asynchronously after its window is available. Read-only CLI inventory never unlocks anything; CLI secret use is lazy. Concurrent requests serialize one vault unlock and share a denied outcome until an explicit retry or session expiry, avoiding password storms. Migration is additive and resumable: validate all inputs and bounds before publishing one vault document; never delete original items. A dedicated cross-process lock protects aggregate read/merge/write. Concurrent vault revisions are revalidated and merged, never overwritten from an old process snapshot.

Use a maximum of 512 entries, 64 KiB per entry, 8 MiB encoded vault payload. Reject malformed/unknown-schema documents without overwriting them. Clear all secret-bearing memory at session expiration, explicit lock and desktop process exit. Retain the existing uncached snapshot semantics: every transaction read still validates source revision; a cache is not permission to ignore external changes.

If a source key changes outside Arca, the session invalidates that entry. This is a credential change, rather than the ordinary repeated-navigation case. Any necessary renewed authorization must be explicit and coalesced; background reads cannot silently launch a sequence of old-item dialogs.

## Validation

No UI tests. Build Rust Core, CLI and desktop. Exercise the session against an in-memory backend with dummy values under isolated HOME/ARCA_HOME: one unlock for multiple Providers and concurrent readers, time limit, new process/cache instance, denial, external replacement/deletion, source save/delete, corrupt vault, concurrent writers, initial import failure, and zero secret output. Preserve unrelated working-tree changes. No commit, push, release or installed-app replacement without explicit delivery authorization.

## Builder-check resolutions

The dedicated vault lock covers authoritative source writes/deletes as well as merge/persistence. Every imported source uses revision/read/revision validation. An update verifies the written bytes with a noninteractive read before binding a revision. Existing aggregate changes merge from a fresh noninteractive read, failing the derived update rather than overwriting a peer. The vault uses an app-only ACL; `/usr/bin/security` remains trusted only on the independent compatibility items. Only Provider/Profile namespaces participate: private file snapshots and operation rollback entries stay separate. No failed unlock falls back to per-item authentication. Runtime name refresh is retired in favor of the aggregate Arca prompt, avoiding metadata changes immediately invalidating a just-imported session.

## Native write-permission compatibility

The unified authorization covers ordinary Arca-managed credential reads. Retained compatibility items can have independent legacy write ACLs. An explicit save/delete preserves the native permission-recovery dialog for that old item, rather than silently failing and offering an ineffective aggregate-only retry. This is part of legacy permission migration, not a navigation-triggered read. The aggregate itself never introduces extra write prompts; it uses UI-fail. Dismissing the global retry notice does not remove recovery: a subsequent session error creates a fresh notice and reachable action.
