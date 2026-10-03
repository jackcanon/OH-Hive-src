# Shared project connector: first implementation

This adds an inbound Model Context Protocol (MCP) endpoint at `/project/mcp` to the
private primary's existing local server when built with Bots support. It reuses the real
project rooms and Bots messages; it creates no parallel task board or chat history.

## Implemented boundary

An owner operating the local database can issue an independent, expiring bearer credential
for an existing named agent and one or more explicitly selected project rooms. The agent
must already belong to those rooms. Credentials cannot grant membership, change policies,
run commands, read the Library, download files, send direct messages, wake other agents,
claim jobs, or mark work complete. Archive, expiry, revocation, ownership and membership
are checked on each operation inside the database transaction. Only token hashes are stored.
Revocation takes effect after any already-running transaction completes.

Three tools:

- `list_project_rooms`: shared rooms and current conversation policy revisions.
- `read_project_updates`: bounded forward paging by server sequence; honors membership's
  history boundary. Message bodies are untrusted source material. It returns text updates,
  author identifiers, kind and timestamp, not attachments or private agent memory.
- `post_project_update`: authenticated agent authorship, required stable request identifier
  and expected policy revision. Inserts into the same Bots messages table shown by the Den.
  It returns an update receipt, NOT a task completion receipt. No deliveries are enqueued.

Identical retries return the original receipt. Reusing an identifier with different content
fails. Message and receipt are committed atomically. A lost network response can therefore
be retried without duplicate messages. A removed/revoked participant cannot use a retry to
regain access. No actor, recipients, credential or arbitrary procedure name is accepted in
these tool arguments.

## Local administration and verification

`project_connector_admin` is a developer example, not the final end-user onboarding flow.
Run against the selected primary's database, never a copied/offline database that forks history.
Do not open a second primary server on that database. Use the already-running primary's
endpoint once the updated core is installed. Granting requires local database authority and
explicit selection of owner, existing agent, existing project room, read or post permission,
expiry and a new private output file. The raw bearer token is written there once, never logged.
The example refuses to overwrite files and revokes a credential if output persistence fails.

```
cargo run -p hive-core --features local-hub,bots --example project_connector_admin -- \
  grant <database> <owner-id> <agent-id> <project-room-id> read 3600 <new-private-file>
cargo run -p hive-core --features local-hub,bots --example project_connector_admin -- \
  revoke <database> <owner-id> <grant-id>
```

Use an authenticated client with `Authorization: Bearer ...`,
`Accept: application/json, text/event-stream`, and the negotiated
`MCP-Protocol-Version`. This stateless endpoint supports protocol versions 2025-06-18 and
2025-03-26, initialization, ping, tool discovery and calls. Notifications/initialized returns
202 with an empty body. It returns ordinary JSON protocol responses; event-stream GET is
unsupported (405). Browser Origin headers are denied. Body size is capped at 64 KiB. It must
stay on the loopback/private bind restrictions of the existing primary.

## Still required before general availability

### Account compatibility and developer connection

Jack selected a personal Plus/Pro ChatGPT account. The currently documented custom
remote connector development flow does not provide that account with the same write
support as Business/Enterprise/Edu. Pro's documented custom connector path is read/fetch;
Plus availability must be checked separately. This is a constraint of that development
flow, not a claim about every published ChatGPT app. Do not promise posting from the
personal ChatGPT chat until the supported distribution route is verified.

Codex supports an authenticated Streamable HTTP connection. For a client running on the
primary Mac, a developer can configure the following after creating a scoped credential
and providing its token through the client's environment. Never paste a token into this
file. This example does not change the user's installed configuration.

```toml
[mcp_servers.lokis_den_projects]
url = "http://127.0.0.1:8787/project/mcp"
bearer_token_env_var = "DEN_PROJECT_CONNECTOR_TOKEN"
enabled_tools = ["list_project_rooms", "read_project_updates", "post_project_update"]
```

Use a read-only credential for a context-only client. A cloud client cannot reach this
loopback address; the authenticated remote gateway and owner consent remain required.
This endpoint does not yet implement the `search`/`fetch` contract expected by some
ChatGPT research connectors. Native tool discovery alone is not evidence of compatibility
with those clients. Validate each actual client before declaring it supported.

References: [Codex connection configuration](https://learn.chatgpt.com/docs/extend/mcp?surface=cli),
[ChatGPT developer mode and custom connectors](https://help.openai.com/en/articles/12584461-developer-mode-and-mcp-apps-in-chatgpt).

### Remaining delivery work

- Owner-facing sign-in and consent through `lokisden.app`, per-client authorization,
  project sharing controls and revocation. The current bearer tool interface is not a
  completed authorization protocol implementation for ChatGPT or Claude remote connectors.
- Authenticated outbound primary connection and remote gateway. Never expose/proxy
  `/local/v1/rpc`, pairing, administration or the entire local server to cloud clients.
- Vendor/client connection verification for the actual accounts. Interactive connectors
  do not wake idle consumer chats or authorize unattended subscription execution.
- Scoped artifact/research access and explicit bounded handoff operations using existing
  execution services. Add them independently with source revision checks, claims/leases,
  cancellation, results and review evidence. Plain posted updates are deliberately not jobs.
- Native project-sharing setup and external participant labels. Existing agent identity
  is reused, but registering an external collaborator must not silently start a runtime.

Tests cover real database operations and a loopback protocol client; no cloud account,
public exposure, private transcript export or production grant is part of these tests.

Protocol reference: https://modelcontextprotocol.io/specification/2025-06-18/basic/transports
