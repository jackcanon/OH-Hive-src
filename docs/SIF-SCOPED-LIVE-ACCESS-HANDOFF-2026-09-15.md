# Audit S-3/S-4: project-scoped live access

Implemented locally; migration, server and web deployment remain pending. This replaces account-JWT delegation for live boards and removes account-JWT delegation for project-overview snapshots.

## Boundaries and contract

`public.hive_live_token_mint(p_project_id, p_server_id)` requires authenticated membership and current project visibility. Returns `{token, expires_at}`. Token is an HMAC-SHA256 capability bound to viewer UUID, project UUID, intended regional node UUID, absolute expiry (at most 300 seconds) and random nonce. The random 32-byte signing key is stored in `hive.live_signing_key`, RLS-enabled with no PUBLIC/anon/authenticated access. The helper SQL functions have no PUBLIC/anon/authenticated execute permission; only narrowly scoped public wrappers are granted. Tickets are sensitive short-lived credentials; do not log query strings.

Minting and redemption currently require an online hub-attested HJM regional server. This retains the interim trust restriction; it does not enable untrusted volunteer servers to receive private project boards. Key rotation is an administrator replacing the one signing secret; all outstanding tickets then fail. There is no exported signing secret, token-to-account-JWT conversion, generic delegated RPC, or write privilege.

`public.hive_project_board_for(raw_key, p_project_id, p_token)` verifies the intended node key, signature, project, server, expiry and current server eligibility. It temporarily binds auth context to the signed viewer, reuses `hive.is_member`, `hive.project_visible` and `hive.project_board`, then restores both JWT context settings on success/error. The requested project must exist and not be deleted. Every redemption rechecks membership and project visibility, so access changes do not wait for five-minute expiry. Node key revocation also prevents redemption. It returns `{board, expires_at}`. It does not let callers pick an unsigned member identity.

## Regional server

The live route now accepts only the scoped-ticket format. It validates by fetching an authorized board **before** upgrading the socket. It never caches/replays another viewer's board, replaces another viewer's credential, or sends raw hub errors. Each subscriber polls independently every two seconds; any hub error ends the connection, and the browser uses direct hub polling. This intentionally spends one read per subscriber in exchange for correct viewer-specific permissions and `my_role`; future sharing must preserve those semantics rather than sharing raw boards.

Hard cap: 64 concurrent authorization attempts/connections per server. HTTP connect/read deadline: 5/10 seconds. Upstream response bound: 8 MiB. Client frame/message bound: 4 KiB. Socket sends time out after five seconds. A ticket-expiry timer closes the socket; no perpetual expired-token poller. Backpressure and failure paths release their slot. Permissions are rechecked on each poll, not an instantaneous external revocation signal.

## Web

Live boards obtain a ticket through the user's ordinary hub connection, send only that ticket to the selected server, and renew before expiry. Missing migration, denied mint, unavailable server, socket errors or timeout fall back to authorized hub reads; retry is delayed 15 seconds. Effect cleanup closes sockets and timers, and ignores stale callbacks. A quiet socket resumes hub polling after 60 seconds.

The projects overview now reads `hive_projects_overview` directly. The old all-project snapshot has no equivalent project-scoped credential and is not an appropriate place to send an account JWT. Snapshot data can also have different privacy semantics. The existing server snapshot endpoint is not removed in this patch, but these web paths no longer call it. Its independent authorization/data-filter audit remains in the queue.

## Verification and rollout

Focused PGlite test uses real pgcrypto and the existing project_visible implementation: signature tamper, expiry/future expiry, wrong server/project/key, invalid account JWT, viewer/member access changes, node revocation, operator changes, auth-context restoration, signing-key/helper SQL grants. The board builder is a fixture; this is not a full Supabase migration replay.

Real loopback WebSocket tests exercise valid joins, invalid join while a legitimate viewer is connected, viewer-specific replies, visibility revocation, ticket expiry, legacy JWT rejection and capacity rejection. No production credentials are used. Production dependency footprint remains free of wasmtime/sysinfo/rusqlite/openssl; WebSocket test dependencies are dev-only.

Deploy the SQL migration `20260915160000_project_scoped_live_tokens.sql` first, then rebuilt regional servers, then the web app. Before rollout verify existing `hive.is_member`, `project_visible`, `project_board`, `verify_node_key`, extensions.pgcrypto and founder-controlled server registration match expected contracts. Never deploy a signing secret to a regional server. Older clients are refused by the new live route and can fall back to hub polling; new clients cannot use old regional live handlers for account authentication.

Live acceptance after deployment: two accounts with different access to a private project; denied account cannot mint; allowed account streams, then loses access and disconnects; expired/tampered/wrong-node ticket fails; project overview and board network requests contain no account JWT sent to regional hosts. Check normal browser session refresh and proxy WebSocket upgrade behavior. No production acceptance or deployment is claimed by local tests.

Final local checks: PostgreSQL/pgcrypto fixture passed; all four hive-server library tests passed (two new real-WebSocket scenarios); web TypeScript and full Next production build passed; normal/build ARM64-musl dependency gate passed; diff whitespace check passed. Known pre-existing unused worker helper and Node 20 deprecation warnings remain. No browser-to-production or complete Supabase schema replay was performed.
