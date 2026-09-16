# Settings reorganization: separating the Den from the Hive

Loki, 2026-09-16. Jack's ask: Loki's Den is the primary product and the broadest audience will
never touch the Hive, so Hive settings should live in their own section rather than interleaved
with Den settings. Plus specific moves, and several questions he explicitly left open.

Today `SettingsView.swift` is 473 lines and **fourteen** top-level tabs: General, Private Fleet,
ChatGPT, Cloud Keys, Connectors, Backend, Media, Trust, Node, Skills, Server, Earnings, Generate,
Feedback. The audit already called it "a 14-tab junk drawer hosting whole feature screens," with
`noteBox` defined four times, `gb()` twice and the provider-label switch three times.

## One structural recommendation, which differs from the brief

Jack's framing is **Den settings vs Hive settings**, and the moves follow from that: Media might
belong to both, Transcribe should serve both, and so on. I'd draw the line one notch differently:

> **Split by "my machine" versus "the community" — and treat a capability as Den-first with an
> optional Hive switch, rather than as something that belongs to both.**

A capability — Transcribe, Images, local models — lives in the Den, and the "offer this to the
Hive" or "use the Hive for this" switch sits **next to the capability**, not in a Hive tab. The
Hive section then holds only what is meaningless until you join: Trust, Node, Server, Earnings.

Why this and not "it belongs to both": a thing that lives in two places is configured in two places
and drifts. Concretely, a user setting up Transcribe should not have to know the Hive exists, and a
user who wants to donate transcription compute should not have to find a second screen to do it.
One home, one switch. It also makes the collapse rule below trivial — nothing in the Hive section
is needed to use the product.

**Corollary worth acting on: the Hive section should be absent, not merely separate, until the user
joins.** That is the strongest possible expression of "a subset of users may never use Hive
settings" — not a tab they ignore, a tab they never see. A single "Join the Hive" entry point is
the whole surface until they do.

## Answers to the open questions

**Backend — don't just hide it, split it.** "Very very very easy local model setup" is a *feature*,
and hiding the raw endpoint editor is only half of delivering it. Backend today is endpoint
plumbing (`llama_url`, `whisper_url`, `comfyui_url`). Split into:

- **Models** — top level, friendly, in the Den: what is installed, what is running, one button to
  get a recommended model, clear status when nothing is available. This is also what per-agent
  model pinning needs a picker to read from (`LOKI-AGENT-PROFILE-BUILDOUT-2026-09-16.md`), so the
  two should be designed together rather than producing two different model lists.
- **Advanced** — raw endpoints, diagnostics, overrides. Hidden by default, one click away.

**Media — a capability, so Den with a Hive switch.** It configures the same whisper/ComfyUI plumbing
Transcribe and Images use. Note there is already a `hosted_media_generation` migration applied in
production, so the Hive side of this exists server-side and the switch has something real to talk
to.

**Transcribe and Images aren't settings at all.** This is the finding worth acting on. Jack wants
Transcribe to be a real feature wired up as a Hive project, and Buzz-style products put these in
the workspace, not in Preferences. They are in Settings today because Settings is where things
landed, which is exactly the junk-drawer problem the audit named.

So: **move Transcribe and Images out of Settings into the main window** as a proper surface, and
leave only their configuration behind. "Generate" becomes **Images**, grouped with Transcribe — Jack
asked for that grouping and it is right, it just belongs one level up. Once Transcribe is a real
surface, "run this on my Mac" versus "dispatch to the Hive" is a control on that surface, which is
the same shape Media needs, built once.

**Evidence for T-1, from the Generate tab itself.** A screenshot of it shows a full creative
workspace inside Preferences: source picker (OpenAI key vs local ComfyUI), prompt field, negative
prompt, a Generate button and a result canvas taking most of the window. Nobody would design that
as a settings pane. It is the clearest single case of the junk-drawer problem and the reason T-1
leads.

The same screenshot surfaces a real bug to fix while moving it: the helper text reads "using your
own API key (added in Settings on the web app)." The key is configured on a **different surface
than the one that spends it** — a user standing in the Den's Generate tab is told to go to the web
app. That is exactly what the Providers tab (T-3) should absorb: keys live where the app that uses
them lives. Worth checking whether the native app can already read and write BYOK keys, or whether
that copy is describing a genuine gap rather than a stale instruction.

**ChatGPT + Cloud Keys merge — agreed**, and name it for what it is. Both are "how this app reaches
a model provider": one is a subscription sign-in, the other is a BYOK key. One tab, two sections.
The provider-label switch that the audit found duplicated three times should collapse into this
tab as it is built.

**Connectors stays** — agreed, and it is about to grow.

**Private Fleet** is "my computers," which is Den by this split, not Hive — worth saying out loud
because the name sounds Hive-ish. It is the primary/secondary selection work from queue item 3.

## Proposed shape

**Den**
General · Models · Providers (ChatGPT + Cloud Keys) · Connectors · Skills · My Computers (Private
Fleet) · Feedback · Advanced (Backend, diagnostics)

**Hive** — one section, hidden until joined
Membership & Trust · Node · Server · Earnings

**Out of Settings entirely**, into the main window
Transcribe · Images (was Generate)

## Queue

Sif owns all of it (`apps/desktop-swift`), after her current security queue. Ordered so each step
ships on its own.

**T-1 — Extract the feature screens.** Transcribe and Images out of Settings into the main window;
Generate renamed Images. Pure move plus rename, no behavior change, and it shrinks the junk drawer
most. Do this first: it is the only item that makes every later one smaller.

**T-2 — Collapse Hive into one section, hidden until joined.** Trust, Node, Server and Earnings
become one Hive section; the whole section is absent for a user who has not joined, replaced by a
single "Join the Hive" entry. This is the item Jack actually asked for and it is worth doing
properly rather than as a tab rename.

**T-3 — Merge ChatGPT and Cloud Keys into Providers**, and collapse the thrice-duplicated
provider-label switch into it.

**T-4 — Split Backend into Models and Advanced.** Models is the friendly local-model surface; it
must be good, because it is the first thing a new user needs and the thing per-agent model pinning
reads from. Coordinate with the agent-profile work rather than shipping a second model list.

**T-5 — Transcribe as a Hive project.** The dual-function piece: local execution on the user's own
Mac, or dispatched to the Hive as a project. Needs a card/job shape for transcription and a control
on the Transcribe surface. Depends on T-1, and the same switch then serves Media, so design it once
for both.

**T-6 — De-duplicate what the reorg exposes.** `noteBox` (four copies), `gb()` (two) and anything
else the extraction surfaces.

Not queued: per-agent model pinning and the agent profile buildout are in their own doc, but T-4
and that work share a model picker and should not produce two.
