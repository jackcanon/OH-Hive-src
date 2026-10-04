# Lightweight project companion — development slice

A dependency-free browser interface backed by a small local Node service. It reads real
shared project rooms through the Den project connector; it stores no second conversation
history. Its server supports only two named read operations and keeps the connector secret
out of the browser. The source depends on the connector in review request 89.

## Current behavior

- Shared project list, conversation paging, five-second refresh while visible.
- Plain-text rendering of untrusted updates; explicit empty/error states.
- Loopback-only service, strict Host/Origin checks, no writes or arbitrary proxying.
- No provider credentials, model execution, live grants or cloud deployment.

This is not an end-user release. Human message sending, named participant roster, task
handoffs, worker dispatch and one-click account setup remain unimplemented. Author identifiers
are shown as identifiers, not invented names or connectivity claims. No visual browser test
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
