# How Loki calculates real effort (tokens + hours), and a suggested approach for Sif

**By:** Claude (Loki), 2026-09-15
**For:** Sif -- Jack asked me to document the method so you can build an equivalent for the
Codex/ChatGPT side, into the same `Halo-src/docs/CONTINUITY.md` **Effort:** line convention.
**Also see:** `Halo-src/scripts/session_effort_report.py` (the actual tool, read alongside this),
`Halo-src/docs/CONTINUITY.md` intro note (2026-09-15, "yet again, later" entry) for how this
landed in the convention.

## 1. Context

Jack's ask: track real tokens and/or hours spent per task, going forward, so he can pull a
report showing what it actually took to build Hive. My first answer (earlier today) was that
neither of us has in-band access to real token/cost numbers, so the fallback was git-commit
size (insertions/deletions) as a proxy. That fallback is still fine for backfilling anything
before today, but it turned out to be wrong as a permanent answer for my side -- I do have
in-band access, I just hadn't looked in the right place. This doc is that place, written up so
you don't have to rediscover it independently, plus what I *don't* know about your side (Codex
app-server / ChatGPT), which is genuinely a gap on my end, not modesty.

## 2. Where my data comes from

Claude Code / Cowork writes a session transcript to disk as newline-delimited JSON:
`~/.claude/projects/<slug>/<session-id>.jsonl` (plus a `subagents/` subfolder if any Task-tool
subagents ran). Every assistant turn in that file that actually called the model carries a real
`usage` block:

```json
"usage": {
  "input_tokens": 2,
  "cache_creation_input_tokens": 29400,
  "cache_read_input_tokens": 89109,
  "output_tokens": 298,
  ...
}
```

Two caveats that matter before you go looking for a Codex equivalent:

- **A resumed session's transcript only reaches back to the last compaction.** If a long
  conversation gets summarized/compacted (as this one has been, more than once), everything
  before that boundary isn't in the file anymore -- so a report is always "since the last
  compaction," not "since the conversation started," unless you're deliberately spanning
  multiple transcript files.
- **This is per-session, not per-task.** One transcript file can span many unrelated pieces of
  work. Nothing here decomposes "how many tokens did the Bots C0 slice cost" from "how many
  tokens did clearing today's blockers cost" within the same session -- it only gives you the
  session-level (or, with the category breakdown below, the per-turn-cause-level) total.

## 3. The weighting formula

Raw token counts alone are misleading: a cache-read token costs a small fraction of a regular
input token, a cache-write token costs somewhat more than a regular input token, and an output
token costs several times more than a regular input token. So instead of reporting four raw
numbers that don't obviously combine into "how much effort was that," I compute one **weighted
effective-token** figure:

```
weighted = input_tokens * 1.0
         + cache_creation_input_tokens * 2.0
         + cache_read_input_tokens * 0.1
         + output_tokens * 5.0
```

These weights (1x / 2x / 0.1x / 5x) are the ratios Claude's own built-in `explain-usage` skill
uses -- they're a *relative* effort measure roughly proportional to what each token type
actually costs on Anthropic's side, **not** a literal token count and **not** a dollar figure.
I want to be precise about that distinction so it doesn't get mistaken for more precision than
it has.

**For your side:** don't reuse these exact numbers. They're specific to Anthropic's current
cache-discount and output-pricing ratios. If OpenAI's pricing for whatever Codex/ChatGPT runtime
you're on has different ratios (it likely does -- cache discounts and output multipliers vary by
provider and model), the honest move is to look up the current published ratios for your model
and build your own weights, or, better, see section 5 below -- there may be a much simpler path
that skips this whole reverse-engineering step for you entirely.

## 4. How I attribute a turn to "what caused it"

Beyond one total number, I wanted to know *where* the tokens went (working on Jack's Mac through
the device bridge? Cmd Work? writing files? just re-reading my own instructions?). Each
transcript entry has a `parentUuid` pointing at whatever came immediately before it, so I walk
that chain backward from each assistant turn:

1. If the turn carries an explicit `attributionMcpServer` field, that's the answer directly (an
   MCP connector call).
2. Otherwise, walk to the parent entry. If it's a `user`-type entry containing a `tool_result`
   block, I look up which `tool_use` block (by id) that result answers, and that tool's name is
   the cause.
3. Some tool results render as a separate `attachment`-type entry instead of an inline
   `tool_result` (file reads, in my transcript format) -- same idea, different shape.
4. Some `attachment` entries aren't tool output at all -- they're the standing system
   prompt / tool list / skill list / agent list being (re-)injected into context. I tag those
   as "instructions" rather than attributing them to whatever tool call happened to trigger the
   refresh.
5. One attachment type (`total_tokens_reminder` in my transcripts -- a small "N tokens left"
   marker inserted after every tool result) isn't a real cause either; I skip through it to
   whatever it's really sitting on top of.
6. If none of that resolves (e.g. a turn responding directly to a plain user message with no
   tool involved), it falls through to a residual "direct message" bucket.

This is the part most likely to need real rework on your side rather than direct reuse -- it
depends entirely on the shape of whatever log/transcript format Codex's tooling actually
produces, which I haven't seen. The principle (walk back to find what specifically got added to
context right before this turn, and bucket by that) should transfer even if the field names
don't.

## 5. Wall-clock span

Simplest part: take every timestamp in the file, and report `max - min` as the span the
transcript covers. This is *calendar* time, not focused effort -- a session left idle mid-task
inflates it same as active work would. If your session logs carry timestamps (they almost
certainly do), this part should be close to a direct port.

## 6. Suggested approach for you

In rough order of how much work each is, cheapest first:

1. **Check whether Codex CLI or the app-server already surfaces real usage/cost natively** --
   a `/usage` or `/cost` command, a status line, a `--json`/telemetry output mode, or something
   in whatever config/log directory it keeps. If it does, that's strictly better than anything
   below: it's the provider's own accounting, not a reverse-engineered approximation, and it's
   exactly what the CONTINUITY.md convention already says to prefer ("if an agent's own tool
   gives real numbers, report those instead of a proxy"). I'd start here before building
   anything.
2. **If not, look for a session log/transcript file** analogous to mine -- something that
   records each API turn with a token-usage breakdown (OpenAI's API responses typically include
   `usage.prompt_tokens` / `completion_tokens`, and separately a cached-tokens count when prompt
   caching is in play). If Codex's app-server persists that anywhere per-session, the same
   three pieces apply: (a) a weighting formula using OpenAI's actual current published ratios
   for your model, not mine, (b) a "what caused this turn" attribution if the log format
   supports it (skip this if it's not there -- a flat weighted total without the breakdown is
   still useful on its own), (c) wall-clock span from the log's own timestamps.
3. **If neither exists**, the honest fallback is what the convention already had before today:
   git commits + insertions/deletions for your work, self-reported. Not as good, but real and
   defensible, and that's fine -- it's what backfilled Sept 4 onward already.

Whatever you land on, the deliverable that actually matters to Jack is small: a number (or two --
tokens and hours) to drop into your own Effort lines in `CONTINUITY.md`, in roughly the same
"~X effective tokens, ~Y hours" shape mine now use, so a reader can tell at a glance what each
entry cost without needing to know the methodology behind it.

## 7. One honesty flag, worth stating plainly

Even once both of us have real per-agent numbers, **they are not the same currency.** My
weighted-token figure reflects Anthropic's pricing ratios; yours (if built from OpenAI's) would
reflect OpenAI's, which differ. Please don't let a future report (mine, yours, or Jack's) add
"Loki's tokens" and "Sif's tokens" together as if 1 unit of one equals 1 unit of the other, or
present a side-by-side comparison as apples-to-apples without that caveat attached. They're each
real within their own accounting, not directly comparable across providers. If Jack wants a
genuinely unified cross-provider cost figure later, that needs an actual dollar conversion using
each provider's published rates, not a token-weight comparison -- a different, harder project
than what either of us has built so far.
