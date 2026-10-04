# Lightweight project companion — development slice

A dependency-free browser interface backed by a small local Node service. It reads real
shared project rooms through the Den project connector; it stores no second conversation
history. Its server supports only two named read operations and keeps the connector secret
out of the browser. The source depends on the connector in review request 89.

## Current behavior

- Shared project list, conversation paging, five-second refresh while visible.
- Plain-text rendering of untrusted updates; explicit empty/error states.
- Loopback-only service, strict Host/Origin checks, explicit human posting only, no arbitrary proxying.
- No provider credentials, model execution, live grants or cloud deployment.

This is not an end-user release. Human messaging and the current named roster are implemented. Agent availability remains
unverified; task handoffs, worker dispatch and one-click account setup remain unimplemented.
The composer requires an owner human grant, never an agent credential. No visual browser test
or live Den connection has been performed yet.

## Developer setup

Node 20 or later; no package installation required. Use the updated primary's loopback
project connector and an explicitly scoped credential from its trusted administration tool.
Pass its token using the process environment, not a source file, shared note or URL.

```
node apps/companion/server.mjs
node --test apps/companion/server.test.mjs
```

Required environment: `DEN_PROJECT_CONNECTOR_TOKEN`. Optional `DEN_PROJECT_ENDPOINT`
(default `http://127.0.0.1:8787/project/mcp`), `DEN_COMPANION_PORT` (default 4317).
This development bridge directly uses the connector's stateless named-tool contract;
full external-client lifecycle and authorization are separate work.

A future packaged background service supplies this configuration after owner sign-in;
end users should not handle addresses or tokens. It must be usable with a headless project
service as well as the full Den. Neither existing consumer subscriptions nor a shared chat
alone provide an unattended worker.

Issue a human credential with the trusted administration example using `human` in place of
the existing agent identifier. It acts only as the selected project owner, requires current
room membership, and grants no automatic agent delivery. Message retries retain their
request identity until confirmed; definitive validation/access rejection permits editing.

## Background assistant connection

The optional browser interface is not needed for local assistants. A standard-input/output
Model Context Protocol adapter is available at `connector/stdio.mjs`:

```
node apps/companion/connector/stdio.mjs
```

Configure a compatible local tool client to launch that command, with the same endpoint and
an **agent** credential supplied through its process environment. The adapter refuses a
human credential, negotiates the supported protocol, and exposes only scoped project tools.
It neither starts a model nor dispatches jobs. Standard output contains protocol messages
only; connection failures are sanitized. Input and responses are bounded; slow output applies
backpressure. Notifications do not create work.

This is a developer connection, not a completed one-click setup. Actual Claude, Hermes and
Codex client configuration and account testing are still pending. Browser-hosted ChatGPT
requires its supported remote connection route; it cannot launch this local process.
A packaged installer must resolve the executable and script paths and supply owner-approved
credentials without asking end users to copy secrets into configuration files.

### Address an agent by name

The optional companion composer defaults to **Whole team · shared note**. Selecting a
named agent sends an addressed request through the same project history. The server checks
current project membership, creates the routing envelope, and uses the message request
identity as the event identity. An uncertain save freezes both recipient and text until the
same request is retried. Addressed requests and replies display their text and recipient name
rather than raw routing data.

Saving a request does not prove an agent is available. Its explicitly configured watcher must
be running and authorized for that author and project. Shared notes do not start agents.
There is no automatic assignment of execution permissions, and the current adapters are
bounded, message-only follow-ups. Browser behavior is covered with isolated document fixtures;
visual acceptance and a real signed-in exchange remain pending.
