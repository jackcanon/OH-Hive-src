# ADR-039: A read-only debug surface for external assistants (Claude Cowork, ChatGPT, Claude Code)

Status: Proposed — design only, not enabled or implemented.

## Context

Jack asked for something specific on 2026-09-19, after computer-use automation against a running
Loki's Den dev build failed twice in a row (`computer_app_list_windows` returned nothing against
an unsigned/dev-path build, then a full-screen-control session got interrupted by an incoming
call): *"I need for you to have full visibility on Loki's Den. Like we need to wire up full
control of the app for both you (Claude Cowork) and ChatGPT. We need to be able to troubleshoot
issues without having to request computer usage. So what is the best way to accomplish that?"*

Today an external assistant has exactly two ways to see what a Den instance is actually doing:

1. **Screen automation.** Slow, needs a fresh permission grant per session, and fails outright
   against an unsigned/dev build because the accessibility bridge can't enumerate its window
   (`pid: -1`, no windows returned). It also means Jack's screen gets taken over to answer a
   question like "is the card actually stuck, or just slow."
2. **SSH plus raw tools.** What I used tonight to reach Nidavellir: `ssh`, then hand constructed
   `sqlite3` queries or `hive` CLI calls against files and ports I had to go find first. This
   works but it is bespoke every time, has no audit trail, and only I can do it — ChatGPT has no
   equivalent path at all, so today's setup is asymmetric between the two assistants Jack named.

Neither is "visibility." Both are workarounds.

Meanwhile the pieces this needs mostly already exist:

- **The local hub already has an inspection method.** `LocalHubStore::inspect()`
  (`crates/ohhive-core/src/local_hub/mod.rs`) returns every card, its status and reason, every
  card output, and the activity count, in one call. Its doc comment is explicit about why it
  isn't reachable today: *"Owner-only local inspection. No anonymous/network read endpoint."*
  That is the right restriction for an *anonymous* endpoint. It is not a restriction on an
  *authenticated* one — nothing about `inspect()` requires it to stay off the wire, only that
  whoever calls it must already be a party the vault trusts.
- **The local hub already has an authenticated RPC transport.** `local_hub/transport.rs` serves
  `/local/v1/rpc` over HTTP, bearer-authenticated with the same per-node key `hive pair` issues,
  bound to loopback or private-LAN only (`local_ip()`, enforced in both `serve()` and the
  `RemoteLocalHub` client). This is the exact "Connection details" endpoint I saw in Settings →
  Private Fleet tonight (`http://192.168.1.7:8767`) mid-way through trying to pair Nidavellir in.
  Every RPC method is one `match` arm in `dispatch()`.
- **af2effb7 is already building the thing a debug surface would otherwise duplicate.** That
  ticket's whole point is that an MCP tool call and a `read_file`/`run_command` call must be
  authorized by the *same* host dispatcher, so a registry-enabled check and an agent-tool-policy
  check never split into two systems that disagree. A debug surface that invents its own
  auth/authorization path would be exactly that mistake a third time.

So the question isn't "how do we build visibility" — it's "how do we expose what already exists,
to a caller that isn't a Den GUI, without opening a second front door."

Related but different ADRs, checked before drafting this one:

- **ADR-031 (external agent adapter, Sif, proposed).** Lets an external caller *submit and follow
  cards* — give the Den work. That's the opposite direction: an external assistant handing work
  in, not looking at what's already running. `hive card submit/status/await` (ADR-030, which
  ADR-031 extends) is real and shipped; ADR-031's fuller JSON contract is design-only.
- **ADR-033/034 (subscription coordinators).** The Den running ChatGPT/Copilot/Grok *as a
  worker*, authenticated by their own subscriptions. Also the opposite direction: Hive-managed
  outbound use of another vendor's model, not an external assistant reading Hive's own state.
  ADR-033 is explicit that Claude itself is out of scope for subscription-auth coordination,
  under Anthropic's Agent SDK terms — irrelevant to this ADR, since nothing here asks Claude to
  authenticate as a paid ChatGPT/Copilot seat, only to read a vault it's already paired to.

This ADR is neither of those. It's an external assistant *observing* a Den it's already
been given a key to, nothing more.

## Decision

Add a small set of **read-only** `debug_*` methods to the existing `dispatch()` match in
`local_hub/transport.rs`, reachable over the same `/local/v1/rpc` endpoint, authenticated by the
same bearer node key every other method already requires. No new port, no new auth mechanism, no
new binding rule — `local_ip()`'s loopback/private-LAN restriction applies exactly as it does
today.

**Pairing.** An external assistant gets a key the same way a Fleet node does: `hive pair` /
the existing pairing-code flow, or the desktop app's Private Fleet pairing screen. Nothing new to
build there. If a lighter-weight "observer" node role turns out to be wanted later (so a debug
key can't accidentally claim cards), that's a follow-up, not a blocker — v1 uses whatever key the
assistant already has.

**Methods (v1, all read-only):**

| Method | Backed by | Returns |
|---|---|---|
| `debug_snapshot` | `LocalHubStore::inspect()`, made reachable over RPC for an authenticated caller instead of only the owner-process caller | cards, card outputs, activity count |
| `debug_node_status` | existing `nodes` row for the session's own node | checked-in state, caps, tools_level |
| `debug_agents_list` | `bots_agents_list` (already exists, already authenticated) | every Bots agent, unfiltered |
| `debug_agent_tool_policy_get` | `bots_agent_tool_policy_get` (already exists) | one agent's tool policy, rendered through the **granted / denied / granted-but-inert** three-state model this vault already uses elsewhere, not a binary |
| `debug_mcp_servers_list` | new: joins `mcp_servers` (registry) against `bots_agent_tool_policies` (grants) | every configured MCP server, its enabled/disabled state, and which agents' policies reference it — the exact granted-but-inert visibility af2effb7 needs for its own UI, reused here |
| `debug_activity_recent` | `activity` table, bounded page | recent host-dispatched actions with their outcome, for "what did the agent actually do" without screen-sharing the transcript |

Every one of these is additive to `dispatch()`'s existing match arms — same function, same file,
same auth path. There is no separate "debug service."

**What this deliberately does NOT do:**

- **No mutation.** No `debug_*` method restarts an agent, changes a policy, kills a card, or
  writes anything. Jack's ask was to *troubleshoot without taking the screen* — diagnosis, not
  remote control. A mutating debug surface is a second permission system with sharper edges than
  the read-only one and belongs in its own ADR if it's wanted, gated behind the same
  explicit-approval-per-action model the top-level safety rules already require of any
  screen/computer-use action today.
- **No new authorization logic.** `debug_agent_tool_policy_get` calls the *existing* method;
  `debug_mcp_servers_list` reads the *existing* tables af2effb7 is already making authoritative.
  If af2effb7 changes what "enabled" or "granted" means, this surface inherits that change for
  free instead of drifting out of sync with it.
- **No symmetry-breaking between assistants.** Whatever key a Cowork session holds and whatever
  key a ChatGPT session holds go through the identical `/local/v1/rpc` dispatch. Neither gets a
  wider surface than the other; the difference between them is which vendor is asking the
  questions, not what they're allowed to see.

**One deliberate deviation, flagged explicitly:** `transport.rs` currently documents this hub as
having "no ... request logging." I'm proposing `debug_*` calls be the one exception — each one
appended to `activity` the same way `post_activity` already records dispatched actions, tagged
with the calling assistant's node name. The reasoning: once a third party that isn't Jack's own
Den client can read production state, "what did an external assistant look at and when" is
itself something Jack should be able to answer later without having to ask us. This is new
behavior for this hub and I'm not assuming it's uncontroversial — flagging it for Jack/Sif rather
than quietly adding logging to a module whose header explicitly says there isn't any.

## Consequences

- Claude Cowork and ChatGPT (and Claude Code, and any future assistant) get the same diagnostic
  read surface, over plain HTTPS/JSON, the moment they hold a paired node key — no screen-control
  request, no SSH, no bespoke `sqlite3` archaeology.
- Nothing here weakens af2effb7's "one dispatcher" property; it strengthens it, by giving that
  dispatcher one more consumer instead of building a second one.
- The granted/denied/granted-but-inert rendering this ADR reuses for `debug_agent_tool_policy_get`
  and `debug_mcp_servers_list` is exactly the same rendering af2effb7's own acceptance criteria
  require of the Tools-and-access panel — implementing one is most of the work for the other.
- New audit surface (the `activity` logging above) is a real behavior change to a module that
  currently promises none, and needs Jack's sign-off, not just mine, before it ships.

## Alternatives considered

- **Screen automation only (status quo).** Rejected: exactly the problem Jack raised — slow,
  permission-heavy, and doesn't work at all against unsigned/dev builds.
- **SSH/raw-tool access for every assistant (status quo for me, unavailable to ChatGPT).**
  Rejected: bespoke per-session, no audit trail, and asymmetric between the two assistants Jack
  named in the same sentence.
- **A full read-write API for external assistants.** Rejected for v1: collapses "diagnose" and
  "control" into one grant, which is precisely the binary-permission failure mode this vault's
  own conventions (granted/denied/granted-but-inert, one dispatcher not two) exist to avoid.
  Mutating actions get their own ADR if/when wanted.
- **A brand-new debug-only HTTP service, separate from `/local/v1/rpc`.** Rejected: a second
  transport is a second thing to authenticate, bind-restrict, and keep in sync with the first —
  the exact "two permission systems" trap af2effb7's own writeup names by name.

## Open questions for Jack / Sif

1. Is a plain node key enough for `debug_*` v1, or does an external assistant need a distinguishable
   pairing (so a compromised debug key can't also `claim_card`)? I'd default to "ship v1 with the
   plain key, split it later if it turns out to matter" rather than block on a new pairing mode.
2. Does the new `activity` logging for `debug_*` calls need its own opt-out, or is "every debug
   read is logged, no exceptions" the right default?
3. Sequencing relative to af2effb7: I'd land af2effb7's registry/policy split first, then this ADR's
   methods reuse it directly rather than needing their own follow-up pass.
