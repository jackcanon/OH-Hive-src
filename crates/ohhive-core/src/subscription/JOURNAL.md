# Subscription turn journal integration

This is a storage prerequisite for the ADR-034 subscription runner, not an enabled
Copilot chat adapter. It makes no provider calls and is not yet called by the Bots
executor or native diagnostic. Keep this separate from the LocalHub database; its
application ID and schema checks reject unrelated databases.

The trusted host opens one private database and establishes a Binding from verified
owner/account, host, agent, conversation, workspace and policy revision references.
Credentials and transcripts do not belong in the binding or journal. Any change of
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
