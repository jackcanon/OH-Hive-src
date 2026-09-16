# Subscription turn journal integration

This is a storage prerequisite for the ADR-034 subscription runner, not an enabled
Copilot chat adapter. It makes no provider calls and is not yet called by the Bots
executor or native diagnostic. Keep this separate from the LocalHub database; its
application ID and schema checks reject unrelated databases.

The trusted host opens one private database and establishes a Binding from verified
owner/account, host, agent, conversation, workspace and policy revision references.
Credentials do not belong in this database. Bindings contain opaque references; the
version-2 results table holds private reply text for recovery. Any change of
binding requires a new session ID and an explicit context handoff.

Runner sequence:

1. Acquire a session writer lease (1–300 seconds). Renew while active. Every write
   checks writer identity, generation and expiry in a SQLite immediate transaction.
2. Enumerate pending operations before new work. Recover `prepared` from its durable
   Bots input and verify the original hash. Reconcile `delivery_unknown` against
   provider/session evidence. Never automatically replay an uncertain send.
3. Prepare a stable operation ID and SHA-256 of the exact normalized turn envelope
   (context, selected model, tools and relevant policy). Reusing an ID with changed
   input is rejected; another outstanding operation blocks new work.
4. Commit `mark_dispatched` before invoking the provider. **Send only if it returns
   true**. A false return is not authorization to retry; any database error stops
   the send. This intentionally admits an uncertain record if a crash occurs after
   commit but before the actual request, rather than risking duplicate delivery.
5. Persist the provider turn ID as soon as acknowledged. Store the final result in
   the conversation/result store, then call `finish` with its durable receipt
   reference. Terminal records and provider IDs cannot be replaced by a conflicting
   receipt. Conversation writes must themselves be idempotent by operation ID.
6. Release the writer on orderly shutdown. Dispatched work becomes unknown on
   release or expired-writer takeover. Old callbacks are fenced. A crashed writer
   may hold its lease for up to 300 seconds before takeover; no forced early theft.

Limitations to resolve in the runner/broker slice: user cancellation before dispatch,
provider-specific reconciliation, credential expiry and account revocation, durable
result/outbox transaction across stores, retention, and authority migration. This
journal is host-local, not a distributed consensus or cross-host failover mechanism.
Lease time uses the host wall clock; hosts must have sensible time synchronization.
The trusted API cannot prove that a caller actually reconciled a provider result;
that obligation belongs to the adapter. It is not exposed to agents or HTTP callers.

Verified with two independent SQLite connections and disk reopen tests: exclusive
writers, expiry/generation fencing, immutable bindings/input, crash before/after
send intent, persistent provider IDs, unknown-delivery blocking and immutable
terminal receipts. No live provider request is necessary for these tests.

Sif your friendly Codex Agent

## Shared turn runner (next implemented layer)

`runner.rs` now implements the sequence through `TurnRunner`, `SubscriptionRuntime`
and `ResultStore`. It is not yet connected to the Bots executor or a concrete
Copilot runtime adapter. `Envelope` fixes model and exact bounded prompt; the runner
computes its SHA-256 itself. It rejects disabled cloud coordination, a mismatched
provider, oversized input, pre-cancellation and invalid timeouts before sending.

The runner claims a 30-second writer lease, renews every ten seconds during provider
I/O, commits before send, acknowledges the provider ID before result persistence,
and stores the terminal receipt. Replaying a completed operation returns its receipt
without invoking the provider. Timeout/cancellation requests a bounded best-effort
interrupt and releases to delivery_unknown. Dropping the future leaves recovery to
lease expiry. The explicit reconciliation method is read-only at the provider and
never sends again. Even after a successful provider reply, failed persistence leaves
an uncertain operation that must be reconciled rather than regenerated.

The concrete ResultStore must durably deduplicate by session/operation and reject
conflicting text. `DurableResults` now implements this contract in the same private database.
The runtime must correlate by stable operation ID, expose only the authorized
context, disable built-in tools until the broker exists, and avoid its own automatic
retries. Caller must retain the cancellation sender: a closed channel cancels work.
Once a provider response is received, bounded result persistence completes even if
a cancellation arrives during that write, preserving a result that already exists.

Tests use controlled adapters/result stores and real disk journals: reopen replay,
lost acknowledgement, changed input, storage failure, timeout, in-flight cancellation
and consent/provider guards. No real cloud calls were made by these tests.

## Durable replies

`results.rs` opens a second connection to the journal database and uses blocking
workers for SQLite operations. Version 1 upgrades transactionally to version 2.
Replies are immutable by session/operation and require the exact stored binding
and acknowledged provider turn. Identical writes return the original receipt.
Reconciliation checks this store first, so a crash after reply persistence but
before journal completion can recover without provider access or another send.

This is a durable result store, not yet a Bots publishing outbox. Conversation
publication, delivery acknowledgement, retention, and account-bound UI wiring
remain to be integrated. Stored reply text is protected by host file permissions,
not application-level encryption. Callers must use a private application directory
(and appropriate Windows ACLs). A timed-out blocking write may finish; immutable
writes and subsequent recovery handle this without regenerating the response.
