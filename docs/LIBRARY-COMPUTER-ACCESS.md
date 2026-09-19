# Library computer access

Library → select a collection → Computer access lists computers enrolled in this Mac's collection store. The owner can allow or revoke collection reads using switches. IDs are available under Computer identity because display names are not unique. Inactive/revoked computer registrations remain visible so stale grants can be removed; they cannot be newly enabled.

This screen administers collections stored on this Mac. It does not send an administrator operation to a selected remote primary and does not add an HTTP grant endpoint. The existing local-host trust boundary remains. Pairing alone still grants no Library access. Agent tool permissions are a separate required layer; enabling a computer never updates agent tool policies or starts a worker/model.

Revoking blocks future authorized reads. It cannot retract previous tool results, messages or copied material. Changes are immediate, and a failed write does not optimistically change the visible switch. Reload errors are displayed.

Implementation: host-local vault_computer_access/vault_set_computer_access validate collection and enrolled active computer in one transaction; idempotent allow/remove uses existing vault_readers with no migration. UniFFI exposes this only on local HiveNode vault state. Swift adds LibrarySharingView and a collection toolbar entry through HiveStore. No direct database editing by the UI.

Verification: focused core test covers default denial, idempotent enable, read success, collection isolation, same-named-computer isolation, revocation, revoked/unknown target rejection and unknown collection rejection. Build/lint outcomes are recorded in continuity. Installed visual/live two-computer verification remains necessary; no real user grants changed during development.
