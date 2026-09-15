// Hive — chat assistant / project-creation agent (ADR-006 D37–D39, ADR-011 D54, ADR-002 §7).
// Renamed from "interviewer" 2026-09-10 -- same mechanism, plain-chat framing (Jack: most members
// expect a straight chat-with-a-model interface and shouldn't need to know this exists).
//
// Two modes as of 2026-09-12 (Jack: "walk back from having the coordinator interview -- let
// people pick between straight agent chat and using the coordinator to build a project, it needs
// to feel more like a typical claude chat experience"):
//   mode: "chat" (default) -- plain conversation. No create_project_plan tool is even offered, so
//     the model can't steer toward or accidentally trigger project creation; it just talks.
//   mode: "plan" -- the original interview behavior, unchanged: create_project_plan is offered,
//     and the system prompt actively works toward gathering enough to call it. The web app enters
//     this by re-sending the SAME conversation with mode:"plan" once the member clicks "Turn this
//     into a project" -- nothing already said gets lost, it just starts being steered toward a plan.
//   mode: "list_models" (2026-09-15, Jack: the chat composer's model picker "not actually loading
//     with models" -- ProviderModelPicker.swift's stage 2 had shipped 9/13 with only "Default"/
//     free-text "Custom..." since there was no live per-provider model list yet). Short-circuits
//     before any chat/plan machinery: given `provider`, resolves that member's own key for it the
//     same way the chat path does, calls that provider's own models endpoint with it, and returns
//     the live catalog. No messages, no memory, no charge -- this is a pure read against the
//     provider's API using a key this function already legitimately holds; nothing new is exposed
//     that `hive_admin_member_key` didn't already gate.
//
// POST /interview   Authorization: Bearer <member's Supabase JWT>
//   { messages: [{role:'user'|'assistant', content:string}], mode?: 'chat'|'plan' }
// → { reply: string, plan?: ProjectPlan, project_id?: string, charged: number, balance: number }
//   { mode: 'list_models', provider: 'anthropic'|'openai'|'nous' }
// → { models: [{ id: string, label?: string }] }
//
// One Claude call per turn, with `create_project_plan` (ProjectPlan contract, packages/schema/
// project-plan.schema.json) offered only in "plan" mode. When the model has enough, it calls the
// tool; we validate, materialize projects + cards via hive.create_project_from_plan, and charge
// the member at provider cost through the peg. Provider keys never leave the hub.
//
// Keys (2026-09-12, BYOK-only -- Jack: "revert the chat to local, and they can input their own
// api for claude or nous or chatgpt"): this Edge Function now ONLY ever uses a member's own key
// (Anthropic, OpenAI, or Nous, from Supabase Vault via hive_admin_member_key — zero Hive cost).
// There is no hub-funded fallback anymore -- a member with no key of their own gets no path through
// this function at all; the web app routes them to the Hive's local community-compute text pool
// instead (hive.interview_send/poll), which is free and unaffected by this change. Nothing is ever
// charged to a member's wallet from this function now, since only BYO keys reach it.
//
// Two ways in (2026-09-13, Jack: "get the BYOK to swift"): a member's own Supabase session (the
// web app, `Authorization: Bearer <member JWT>`), or a raw node key in the body (the Swift/CLI
// desktop app, which never holds a member session -- same node-key resolution as generate-image
// and the feature-request/bug-report node paths, `hive_admin_verify_node_key` +
// `hive_admin_node_member`). The node-key path always forces mode "chat", never "plan" -- the
// desktop Chat tab (ChatEngine.swift) is a small utility panel, not a project-building surface;
// project creation from a conversation stays a web-only feature, same as Kanban voting.
//
// Persistent memory (2026-09-13, Hermes-agent survey -- see supabase/migrations/
// 20260913010000_chat_memory.sql for the full rationale): a bounded, per-member MEMORY.md/USER.md
// pair (hive.chat_memories) is fetched up front and folded into the system prompt every turn, then
// updated by a small background pass (a second, non-tool completion on the SAME already-succeeded
// BYO key) fired via `EdgeRuntime.waitUntil` after the reply is sent -- never on the request's
// critical path, never failing the turn if it errors. This is the one thing `interview` remembers
// across sessions; everything else about this function is still fully stateless per call.
//
// Secrets: INTERVIEW_MODEL (default claude-sonnet-4-5), INTERVIEW_OPENAI_MODEL (default gpt-5).
// Supabase injects SUPABASE_URL / SERVICE_ROLE / ANON. ANTHROPIC_API_KEY (the old hub fallback
// secret) is no longer read by this function -- it can be left in place harmlessly or removed.
//
// CORS: the web app calls this cross-origin (ohghive.com -> *.supabase.co), so the browser sends
// a preflight OPTIONS request before the real POST, and every response (including error ones)
// needs Access-Control-Allow-Origin or the browser discards it before the caller ever sees a
// status code -- surfacing client-side as a generic "failed to send a request", not a 4xx/5xx.

import { createClient } from "npm:@supabase/supabase-js@2";

const MODEL = Deno.env.get("INTERVIEW_MODEL") ?? "claude-sonnet-4-5";
const OPENAI_MODEL = Deno.env.get("INTERVIEW_OPENAI_MODEL") ?? "gpt-5";
// Nous Portal routes via OpenRouter-style "provider/model" slugs, not raw Hermes model ids --
// "nousresearch/hermes-4-70b" 404s ("retired"), and Nous's own docs say Hermes 4 isn't tuned for
// tool-calling anyway (it's a chat/reasoning model). claude-sonnet-4.6 via the Portal is what Nous
// itself recommends for agentic/tool-calling workloads like this one.
const NOUS_MODEL = Deno.env.get("INTERVIEW_NOUS_MODEL") ?? "anthropic/claude-sonnet-4.6";
const NOUS_BASE_URL = "https://inference-api.nousresearch.com/v1";
// USD per million tokens for the hub's interview model — keep in step with the rate table's peg.
const PRICE = { in: 3.0, out: 15.0 };
// Anthropic server-side web search, USD per request (charged to the member alongside tokens).
const WEB_SEARCH_USD = 0.01;

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "authorization, x-client-info, apikey, content-type",
  "Access-Control-Allow-Methods": "POST, OPTIONS",
};

function json(body: unknown, init?: ResponseInit) {
  return Response.json(body, { ...init, headers: { ...corsHeaders, ...(init?.headers ?? {}) } });
}

const PLAN_SCHEMA = {
  type: "object",
  required: ["schema_version", "title", "goal", "license", "requires_internet", "cards"],
  additionalProperties: false,
  properties: {
    schema_version: { type: "integer", enum: [1] },
    title: { type: "string", minLength: 3, maxLength: 120 },
    goal: { type: "string", minLength: 10 },
    license: {
      type: "object", required: ["kind"], additionalProperties: false,
      properties: { kind: { type: "string", enum: ["owner_only", "open_source"] }, spdx: { type: "string" } },
    },
    requires_internet: { type: "boolean" },
    cards: {
      type: "array", minItems: 1,
      items: {
        type: "object", required: ["key", "title", "modality", "inputs", "acceptance"], additionalProperties: false,
        properties: {
          key: { type: "string", pattern: "^[a-z0-9][a-z0-9-]{1,40}$" },
          title: { type: "string" },
          modality: { type: "string", enum: ["text", "code", "image", "video", "speech", "music"] },
          inputs: { type: "string" },
          deps: { type: "array", items: { type: "string" } },
          acceptance: { type: "string" },
          requires_internet: { type: "boolean" },
          required_capabilities: {
            type: "object", additionalProperties: false,
            properties: {
              model_id: { type: "string" }, min_vram_gb: { type: "number" }, min_ram_gb: { type: "number" },
              tools_level: { type: "string", enum: ["inference_only", "sandboxed_tools"] },
            },
          },
        },
      },
    },
  },
};

// Renders the member's persistent memory (hive.chat_memories) as a system-prompt appendix, or ''
// when both blocks are empty (a brand-new member, or one who's cleared their memory). Framed as
// background context, not something to recite -- the model shouldn't announce "I remember that
// you..." every turn, it should just quietly know it, the same way Hermes' own docs describe it.
function memoryAppendix(memory: { memory_md: string; user_md: string }): string {
  if (!memory.memory_md && !memory.user_md) return "";
  const parts: string[] = [];
  if (memory.user_md) parts.push(`About them: ${memory.user_md}`);
  if (memory.memory_md) parts.push(`Notes from past sessions: ${memory.memory_md}`);
  return `\n\nWhat you already know about this member from earlier sessions (use naturally where relevant; don't recite it back or announce that you "remember" things):\n\n${parts.join("\n\n")}`;
}

// mode: "chat" -- no plan-steering, no tool offered. Just a normal, helpful conversation; if the
// member wants to build something on Hive they'll say so themselves via the "Turn this into a
// project" button, which switches subsequent turns to planSystemPrompt below.
function chatSystemPrompt(memberName: string, webSearch: boolean, memory: { memory_md: string; user_md: string }) {
  return `You are Hive's assistant, talking with ${memberName}. This is a normal conversation — answer questions, help them think something through, write or edit something, explain code, whatever they're after. You're not gathering requirements for anything and there's no hidden agenda.

Hive is an invite-only community compute network where members can also turn a conversation into a project — a kanban of cards that idle member machines run — but that only happens if ${memberName} asks for it or clicks the button for it. Don't steer toward that, don't ask the questions you'd ask to scope a project (audience, license, internet access, etc.), and don't bring up "cards," "the plan," or "the coordinator" unless they do first.${webSearch ? " You can search the web when it would help answer something." : ""}${memoryAppendix(memory)}`;
}

// mode: "plan" -- the original interview behavior.
function planSystemPrompt(capacity: unknown, memberName: string, webSearch: boolean, memory: { memory_md: string; user_md: string }) {
  return `You are Hive's chat assistant, talking with ${memberName}. Just chat normally — you're not running a formal "interview" or intake process, and you should never call it that or make it feel like one. Hive is an invite-only community compute network: members contribute idle computers ("nodes"), earn Honey, and spend it on projects. A project is a kanban of cards; each card is one unit of AI work (text, code, image, video, speech, music) that a node runs on a local open-weight model, with no memory between cards except the outputs of the cards it depends on.

Your job: understand what ${memberName} wants made, the way any good conversation would get there, then call create_project_plan exactly once when — and only when — you know all of:
1. What they want to make, concretely enough that each card has acceptance criteria a stranger could check. Dig for the specifics that change the work: audience, length/format/size, tone or style references, what "done" looks like, what exists already (drafts, assets, brand rules), and constraints (deadline, must-include, must-avoid).
2. Whether the work needs the internet (web fetch, APIs, live data). Ask explicitly. Most creative work does not.
3. License: owner-only, or open source (then which SPDX id — suggest MIT for code, CC-BY-4.0 for media).

How to ask:
- Ask the single most useful question next; two at most. Make each question earn its place — if you can infer it, don't ask it. Offer a sensible default in the question ("I'll assume 60 seconds unless you say otherwise").
- Reflect back what you understood in one line before asking, so corrections are cheap.
- Three or four turns is typical; a clear brief can be one. Never pad with generic questions.
${webSearch ? "- You can search the web when a fact would change the plan (a spec, a format's constraints, what a referenced thing is). Do it silently and use the result; don't narrate searches.\n" : ""}
Rules for the plan:
- 2–8 cards. Each card is one deliverable a single model run can produce. Give every card a stable lowercase key, a task written as a complete instruction to a worker who has read nothing else (restate the brief, the audience, the constraints), and acceptance criteria a reviewer can check.
- Use deps to order cards (a card's inputs may reference upstream keys by name). Put research/outline cards before drafting cards when that improves the result.
- Prefer modalities the Hive can run today. Current capacity: ${JSON.stringify(capacity)}. If they need a modality with zero nodes, say so and still plan it (it will queue).
- Don't set required_capabilities.model_id unless the member names a model.

When you call the tool, also write a one-paragraph reply summarising the plan for the member in plain language. Never mention "the interview," "the plan," "cards," "coordinator," or any other Hive-internal mechanics unless the member brings them up first — from where they're sitting, they just described something and it's getting made.${memoryAppendix(memory)}`;
}

type Msg = { role: "user" | "assistant"; content: string };
type Turn = { text: string; plan: unknown | null; tokens_in: number; tokens_out: number; web_searches: number };

// Provider fetches get a hard timeout so a hung/slow provider fails fast into the next
// fallback candidate instead of stalling the whole interview turn (and a demo along with it).
const PROVIDER_TIMEOUT_MS = 25_000;
async function fetchWithTimeout(url: string, init: RequestInit, label: string): Promise<Response> {
  const ctrl = new AbortController();
  const t = setTimeout(() => ctrl.abort(), PROVIDER_TIMEOUT_MS);
  try {
    return await fetch(url, { ...init, signal: ctrl.signal });
  } catch (e) {
    if (e instanceof Error && e.name === "AbortError") throw new Error(`${label} timed out after ${PROVIDER_TIMEOUT_MS}ms`);
    throw e;
  } finally {
    clearTimeout(t);
  }
}

async function callAnthropic(apiKey: string, system: string, messages: Msg[], webSearch: boolean, includePlanTool: boolean, model?: string): Promise<Turn> {
  const tools: unknown[] = [];
  if (includePlanTool) tools.push({ name: "create_project_plan", description: "Create the project and its kanban cards. Call once, when you have enough to.", input_schema: PLAN_SCHEMA });
  if (webSearch) tools.push({ type: "web_search_20250305", name: "web_search", max_uses: 3 });
  const body: Record<string, unknown> = { model: model || MODEL, max_tokens: 2500, system, messages };
  if (tools.length > 0) body.tools = tools;
  const res = await fetchWithTimeout("https://api.anthropic.com/v1/messages", {
    method: "POST",
    headers: { "content-type": "application/json", "x-api-key": apiKey, "anthropic-version": "2023-06-01" },
    body: JSON.stringify(body),
  }, "anthropic");
  if (!res.ok) throw new Error(`anthropic ${res.status}: ${await res.text()}`);
  const out = await res.json();
  const text = (out.content ?? []).filter((c: { type: string }) => c.type === "text").map((c: { text: string }) => c.text).join("\n").trim();
  const tool = (out.content ?? []).find((c: { type: string; name?: string }) => c.type === "tool_use" && c.name === "create_project_plan");
  return {
    text, plan: tool?.input ?? null,
    tokens_in: out.usage?.input_tokens ?? 0, tokens_out: out.usage?.output_tokens ?? 0,
    web_searches: out.usage?.server_tool_use?.web_search_requests ?? 0,
  };
}

// Shared by OpenAI and any OpenAI-compatible provider (Nous Portal included) -- same request/
// response shape, just a different base URL, model, and error label for logging.
async function callOpenAICompatible(baseUrl: string, model: string, apiKey: string, system: string, messages: Msg[], label: string, includePlanTool: boolean): Promise<Turn> {
  const body: Record<string, unknown> = { model, messages: [{ role: "system", content: system }, ...messages] };
  if (includePlanTool) {
    body.tools = [{ type: "function", function: { name: "create_project_plan", description: "Create the project and its kanban cards. Call once, when you have enough to.", parameters: PLAN_SCHEMA } }];
  }
  const res = await fetchWithTimeout(`${baseUrl}/chat/completions`, {
    method: "POST",
    headers: { "content-type": "application/json", authorization: `Bearer ${apiKey}` },
    body: JSON.stringify(body),
  }, label);
  if (!res.ok) throw new Error(`${label} ${res.status}: ${await res.text()}`);
  const out = await res.json();
  const msg = out.choices?.[0]?.message ?? {};
  const call = (msg.tool_calls ?? []).find((t: { function?: { name: string } }) => t.function?.name === "create_project_plan");
  let plan: unknown | null = null;
  if (call) { try { plan = JSON.parse(call.function.arguments); } catch { plan = null; } }
  return { text: (msg.content ?? "").trim(), plan, tokens_in: out.usage?.prompt_tokens ?? 0, tokens_out: out.usage?.completion_tokens ?? 0, web_searches: 0 };
}

function callOpenAI(apiKey: string, system: string, messages: Msg[], includePlanTool: boolean, model?: string): Promise<Turn> {
  return callOpenAICompatible("https://api.openai.com/v1", model || OPENAI_MODEL, apiKey, system, messages, "openai", includePlanTool);
}

function callNous(apiKey: string, system: string, messages: Msg[], includePlanTool: boolean, model?: string): Promise<Turn> {
  return callOpenAICompatible(NOUS_BASE_URL, model || NOUS_MODEL, apiKey, system, messages, "nous", includePlanTool);
}

// One entry from a provider's own model catalog (mode: "list_models", 2026-09-15). `label`, when
// present, is a friendlier display name -- only Anthropic's endpoint returns one.
type ProviderModel = { id: string; label?: string };

// Anthropic's own `/v1/models` (not a chat/completions call -- a plain catalog read, same auth
// headers as `callAnthropic`). Response shape: `{ data: [{ id, display_name, created_at, ... }],
// has_more, first_id, last_id }`. `limit=100` in one page is plenty here -- this is a picker's
// dropdown, not an exhaustive archive browse, and the catalog isn't large enough yet to need the
// has_more/last_id cursor. Every model Anthropic lists is chat-capable, so no filtering needed.
async function listAnthropicModels(apiKey: string): Promise<ProviderModel[]> {
  const res = await fetchWithTimeout("https://api.anthropic.com/v1/models?limit=100", {
    method: "GET",
    headers: { "x-api-key": apiKey, "anthropic-version": "2023-06-01" },
  }, "anthropic models");
  if (!res.ok) throw new Error(`anthropic ${res.status}: ${await res.text()}`);
  const out = await res.json();
  const data: unknown[] = Array.isArray(out.data) ? out.data : [];
  return data.map((m) => {
    const rec = m as { id: string; display_name?: string };
    return { id: rec.id, label: rec.display_name };
  });
}

// Non-chat model families that would just clutter an OpenAI-compatible model picker -- a
// heuristic substring denylist, not an exhaustive one. OpenAI's `/v1/models` lists everything the
// key can see (embeddings, TTS, Whisper, legacy completion models included), and Nous Portal is
// assumed OpenAI-compatible enough to share this same shape (unverified against Nous's own docs
// as of this writing -- if their `/models` response differs, `listOpenAICompatibleModels` below
// still degrades to an empty list rather than throwing, so the picker just falls back to
// Default/Custom for that provider instead of breaking the whole request).
const NON_CHAT_MODEL_MARKERS = ["whisper", "tts", "embedding", "moderation", "dall-e", "davinci", "babbage", "curie", "ada"];

async function listOpenAICompatibleModels(baseUrl: string, apiKey: string, label: string): Promise<ProviderModel[]> {
  const res = await fetchWithTimeout(`${baseUrl}/models`, {
    method: "GET",
    headers: { authorization: `Bearer ${apiKey}` },
  }, `${label} models`);
  if (!res.ok) throw new Error(`${label} ${res.status}: ${await res.text()}`);
  const out = await res.json();
  const data: unknown[] = Array.isArray(out.data) ? out.data : [];
  return data
    .map((m) => m as { id: string; created?: number })
    .filter((m) => typeof m.id === "string" && !NON_CHAT_MODEL_MARKERS.some((marker) => m.id.toLowerCase().includes(marker)))
    // Newest first when the provider reports `created` (a unix timestamp) -- falls back to
    // whatever order the provider returned when it doesn't (missing values sort as 0, to the end).
    .sort((a, b) => (b.created ?? 0) - (a.created ?? 0))
    .map((m) => ({ id: m.id }));
}

function listProviderModels(provider: "anthropic" | "openai" | "nous", apiKey: string): Promise<ProviderModel[]> {
  if (provider === "anthropic") return listAnthropicModels(apiKey);
  if (provider === "openai") return listOpenAICompatibleModels("https://api.openai.com/v1", apiKey, "openai");
  return listOpenAICompatibleModels(NOUS_BASE_URL, apiKey, "nous");
}

// `model` is the member's own per-provider override (hive.member_keys.preferred_model, 2026-09-13:
// Jack, "if we've added an api cloud model then we should be able to pick which cloud model we want
// to run" -- mirrors the existing local HIVE_MODEL picker). Undefined/empty means "use this
// function's configured default" (MODEL / OPENAI_MODEL / NOUS_MODEL) -- unchanged behavior for every
// member who hasn't set one.
type Brain = { provider: "anthropic" | "openai" | "nous"; key: string; byo: boolean; model?: string };
type Memory = { memory_md: string; user_md: string };

// The background memory-review pass (2026-09-13, see this file's header + migrations/
// 20260913010000_chat_memory.sql). One extra non-tool completion on the SAME key that just
// succeeded for the real reply -- billed to the member's own BYOK provider, same as the turn
// itself. Deliberately terse instructions, since this call's only job is "decide what's worth
// keeping, emit updated JSON" -- it never sees the tool-calling/plan machinery the main call does.
function memoryReviewSystemPrompt(current: Memory): string {
  return `You maintain two small persistent memory blocks for Hive's chat assistant about one member, carried into all of their future sessions.

MEMORY -- environment/project facts and lessons learned, currently ${current.memory_md.length}/2200 chars:
"""
${current.memory_md}
"""

USER -- who they are: role, preferences, communication style, currently ${current.user_md.length}/1375 chars:
"""
${current.user_md}
"""

Given the exchange below, decide whether anything is worth remembering long-term. Most exchanges teach nothing worth keeping -- skip small talk, one-off questions, and anything easily re-derived. When something IS worth keeping (a stated preference, a corrected assumption, a project detail, a completed piece of work), fold it in, consolidating or dropping stale/less useful entries to stay within the character limits above.

Reply with ONLY a JSON object, no markdown fence, no commentary: {"memory": "<full updated text, or the current text unchanged>", "user": "<full updated text, or the current text unchanged>"}.`;
}

// Fires the review and, if it produced a real change, writes it -- all off the request's critical
// path (see callers: scheduled via EdgeRuntime.waitUntil after the real reply is already on its
// way back). Any failure here (bad JSON, provider error, RPC error) is logged and swallowed --
// memory is a nice-to-have, never a reason a chat turn should look like it failed.
async function updateMemoryBackground(admin: ReturnType<typeof createClient>, brain: Brain, memberId: string, current: Memory, lastUserText: string, replyText: string): Promise<void> {
  const system = memoryReviewSystemPrompt(current);
  const reviewMessages: Msg[] = [{ role: "user", content: `Member said: ${lastUserText}\n\nAssistant replied: ${replyText}` }];
  const result = brain.provider === "anthropic" ? await callAnthropic(brain.key, system, reviewMessages, false, false, brain.model)
    : brain.provider === "openai" ? await callOpenAI(brain.key, system, reviewMessages, false, brain.model)
    : await callNous(brain.key, system, reviewMessages, false, brain.model);
  const cleaned = result.text.trim().replace(/^```(?:json)?\s*/i, "").replace(/```\s*$/, "").trim();
  let parsed: { memory?: unknown; user?: unknown };
  try {
    parsed = JSON.parse(cleaned);
  } catch {
    console.error("memory_review: model did not return valid JSON, skipping:", cleaned.slice(0, 200));
    return;
  }
  const nextMemory = typeof parsed.memory === "string" ? parsed.memory.slice(0, 2200) : current.memory_md;
  const nextUser = typeof parsed.user === "string" ? parsed.user.slice(0, 1375) : current.user_md;
  if (nextMemory === current.memory_md && nextUser === current.user_md) return; // nothing worth writing
  const { error } = await admin.rpc("hive_admin_chat_memory_set", { p_member: memberId, p_memory_md: nextMemory, p_user_md: nextUser });
  if (error) console.error("hive_admin_chat_memory_set failed:", error);
}

// Schedules the above without delaying the response. `EdgeRuntime.waitUntil` (Supabase's Deno
// Deploy runtime) keeps the function instance alive after the response is sent just long enough
// for this to finish; if it's ever unavailable (e.g. local `supabase functions serve`), fall back
// to a plain fire-and-forget so a chat turn never blocks on it either way.
function scheduleMemoryUpdate(admin: ReturnType<typeof createClient>, brain: Brain, memberId: string, current: Memory, lastUserText: string, replyText: string): void {
  const work = updateMemoryBackground(admin, brain, memberId, current, lastUserText, replyText)
    .catch((e) => console.error("memory_update_failed:", e));
  const rt = (globalThis as unknown as { EdgeRuntime?: { waitUntil?: (p: Promise<unknown>) => void } }).EdgeRuntime;
  if (rt?.waitUntil) rt.waitUntil(work); else void work;
}

Deno.serve(async (req) => {
  if (req.method === "OPTIONS") return new Response("ok", { headers: corsHeaders });
  if (req.method !== "POST") return new Response("POST only", { status: 405, headers: corsHeaders });

  const url = Deno.env.get("SUPABASE_URL")!;
  const admin = createClient(url, Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!);
  const body = await req.json().catch(() => ({}));

  const auth = req.headers.get("Authorization") ?? "";
  const userClient = createClient(url, Deno.env.get("SUPABASE_ANON_KEY")!, { global: { headers: { Authorization: auth } } });
  const { data: { user } } = await userClient.auth.getUser();

  let memberId: string;
  let forceChatMode = false;
  if (user) {
    const { data: active } = await admin.rpc("hive_admin_member_active", { p_member: user.id });
    if (!active) return json({ error: "not_a_hive_member" }, { status: 403 });
    memberId = user.id;
  } else {
    // No real member session on this Authorization header (e.g. the Swift/CLI desktop app,
    // which only ever sends the anon key) -- fall back to a raw node key in the body.
    const rawKey = typeof body.raw_key === "string" ? body.raw_key : "";
    if (!rawKey) return json({ error: "unauthenticated" }, { status: 401 });
    const { data: nodeId, error: nerr } = await admin.rpc("hive_admin_verify_node_key", { p_raw_key: rawKey });
    if (nerr) return json({ error: "verify_failed", detail: nerr.message }, { status: 500 });
    if (!nodeId) return json({ error: "invalid_or_revoked_node_key" }, { status: 401 });
    const { data: mid, error: merr } = await admin.rpc("hive_admin_node_member", { p_node_id: nodeId });
    if (merr) return json({ error: "member_lookup_failed", detail: merr.message }, { status: 500 });
    if (!mid) return json({ error: "no_member_for_node" }, { status: 403 });
    memberId = mid;
    forceChatMode = true;
  }

  // mode: "list_models" -- short-circuits before any chat/plan machinery (see this file's header
  // note). Not gated on forceChatMode the way "plan" is: listing models is read-only and has
  // nothing to do with project creation, so both the web app and the desktop node path can use it.
  if (body.mode === "list_models") {
    const provider = typeof body.provider === "string" ? body.provider : "";
    if (provider !== "anthropic" && provider !== "openai" && provider !== "nous") {
      return json({ error: "unknown_provider" }, { status: 400 });
    }
    const { data: key, error: keyErr } = await admin.rpc("hive_admin_member_key", { p_member: memberId, p_provider: provider });
    if (keyErr) {
      console.error(`hive_admin_member_key(${provider}) failed:`, keyErr);
      return json({ error: "key_lookup_failed", detail: keyErr.message }, { status: 500 });
    }
    if (!key) return json({ error: "provider_key_not_configured", detail: `no ${provider} key on file` }, { status: 409 });
    try {
      const models = await listProviderModels(provider, key);
      return json({ models });
    } catch (e) {
      console.error(`list_models_failed (${provider}):`, e);
      return json({ error: "list_models_failed", detail: String(e) }, { status: 502 });
    }
  }

  const messages: Msg[] = Array.isArray(body.messages) ? body.messages : [];
  if (messages.length === 0 || messages[messages.length - 1].role !== "user") {
    return json({ error: "messages must end with a user turn" }, { status: 400 });
  }
  const mode: "chat" | "plan" = !forceChatMode && body.mode === "plan" ? "plan" : "chat";
  const includePlanTool = mode === "plan";

  // Which brain: the member's own key first (free to the Hive), then the hub's.
  const [anthropicRes, openaiRes, nousRes, cfgRes, modelsRes] = await Promise.all([
    admin.rpc("hive_admin_member_key", { p_member: memberId, p_provider: "anthropic" }),
    admin.rpc("hive_admin_member_key", { p_member: memberId, p_provider: "openai" }),
    admin.rpc("hive_admin_member_key", { p_member: memberId, p_provider: "nous" }),
    admin.rpc("hive_admin_setting", { p_key: "interview_web_search" }),
    admin.rpc("hive_admin_member_models", { p_member: memberId }),
  ]);
  // These RPCs fail closed (silently, as far as the member sees) on a permission or query error --
  // log so a misconfigured grant shows up in function_logs instead of masquerading as "no key set".
  if (anthropicRes.error) console.error("hive_admin_member_key(anthropic) failed:", anthropicRes.error);
  if (openaiRes.error) console.error("hive_admin_member_key(openai) failed:", openaiRes.error);
  if (nousRes.error) console.error("hive_admin_member_key(nous) failed:", nousRes.error);
  if (cfgRes.error) console.error("hive_admin_setting(interview_web_search) failed:", cfgRes.error);
  if (modelsRes.error) console.error("hive_admin_member_models failed:", modelsRes.error);
  const byoAnthropic = anthropicRes.data;
  const byoOpenAI = openaiRes.data;
  const byoNous = nousRes.data;
  const cfg = cfgRes.data;
  const webSearch = cfg !== false && cfg !== "false";
  // Per-provider model override (2026-09-13, see hive.member_keys.preferred_model) -- a missing/
  // failed lookup just means everyone gets this function's configured default, same as before this
  // feature existed.
  const models: Record<string, string | undefined> = (modelsRes.data as Record<string, string>) ?? {};
  // Try every configured BYO key in priority order rather than committing to the first one found --
  // a single bad key (e.g. an unscoped Anthropic key) shouldn't block the turn when another usable
  // key is on file. No hub fallback (2026-09-12): every "byo" here is always true.
  let candidates: Brain[] = [
    byoAnthropic && { provider: "anthropic" as const, key: byoAnthropic, byo: true, model: models.anthropic },
    byoOpenAI && { provider: "openai" as const, key: byoOpenAI, byo: true, model: models.openai },
    byoNous && { provider: "nous" as const, key: byoNous, byo: true, model: models.nous },
  ].filter((b): b is Brain => Boolean(b));
  if (candidates.length === 0) return json({ error: "no_byo_key", detail: "no API key on file — add one in Settings, or use local chat instead" }, { status: 503 });

  // An explicit provider choice from the client (2026-09-13, the chat composer's two-stage
  // provider-then-model picker) narrows to exactly that provider instead of the usual
  // try-every-configured-key-in-priority-order fallback above -- a member who picked "OpenAI"
  // shouldn't silently get an Anthropic reply just because an Anthropic key also happens to be on
  // file. `model`, if given, overrides that one candidate's saved `preferred_model` for this turn
  // only (doesn't touch the stored preference -- that's still `hive.member_key_set_model`'s job).
  const requestedProvider = typeof body.provider === "string" ? body.provider : null;
  const requestedModel = typeof body.model === "string" && body.model.trim() ? body.model.trim() : null;
  if (requestedProvider) {
    if (!["anthropic", "openai", "nous"].includes(requestedProvider)) {
      return json({ error: "unknown_provider" }, { status: 400 });
    }
    const narrowed = candidates
      .filter((c) => c.provider === requestedProvider)
      .map((c) => (requestedModel ? { ...c, model: requestedModel } : c));
    if (narrowed.length === 0) {
      return json({ error: "provider_key_not_configured", detail: `no ${requestedProvider} key on file` }, { status: 409 });
    }
    candidates = narrowed;
  }

  const { data: capacity } = await admin.rpc("hive_capacity_summary");
  const { data: prof } = await admin.from("profiles").select("display_name").eq("id", memberId).maybeSingle();
  const { data: memoryData, error: memErr } = await admin.rpc("hive_admin_chat_memory_get", { p_member: memberId });
  if (memErr) console.error("hive_admin_chat_memory_get failed:", memErr);
  const memory: { memory_md: string; user_md: string } = memoryData ?? { memory_md: "", user_md: "" };

  let brain: Brain | null = null;
  let turn: Turn | null = null;
  let lastError: unknown = null;
  for (const candidate of candidates) {
    const anthropicWebSearch = candidate.provider === "anthropic" && webSearch;
    const system = mode === "chat"
      ? chatSystemPrompt(prof?.display_name ?? "the member", anthropicWebSearch, memory)
      : planSystemPrompt(capacity, prof?.display_name ?? "the member", anthropicWebSearch, memory);
    try {
      turn = candidate.provider === "anthropic" ? await callAnthropic(candidate.key, system, messages, webSearch, includePlanTool, candidate.model)
        : candidate.provider === "openai" ? await callOpenAI(candidate.key, system, messages, includePlanTool, candidate.model)
        : await callNous(candidate.key, system, messages, includePlanTool, candidate.model);
      brain = candidate;
      break;
    } catch (e) {
      console.error(`provider_error (${candidate.provider}, byo=${candidate.byo}), trying next candidate if any:`, e);
      lastError = e;
    }
  }
  if (!brain || !turn) {
    return json({ error: "provider_error", detail: String(lastError), byo: candidates[candidates.length - 1]?.byo ?? false }, { status: 502 });
  }

  // Charge only when the hub paid -- dead as of 2026-09-12 (every candidate is byo now), kept as a
  // no-op safety net rather than deleted outright, in case a hub-funded provider path ever returns.
  let charge: { charged?: number; balance?: number } | null = null;
  if (!brain.byo) {
    const searchUsd = turn.web_searches * WEB_SEARCH_USD;
    const { data } = await admin.rpc("hive_admin_charge_interview", {
      p_member: memberId, p_tokens_in: turn.tokens_in, p_tokens_out: turn.tokens_out,
      p_usd_in_per_m: PRICE.in, p_usd_out_per_m: PRICE.out + (turn.tokens_out > 0 ? (searchUsd * 1e6) / turn.tokens_out : 0),
      p_memo: `${mode} turn (${MODEL}${turn.web_searches ? `, ${turn.web_searches} web search${turn.web_searches === 1 ? "" : "es"}` : ""})`,
    });
    charge = data;
  }
  const usage = { tokens_in: turn.tokens_in, tokens_out: turn.tokens_out, web_searches: turn.web_searches };
  const meta = { charged: charge?.charged ?? 0, balance: charge?.balance ?? null, usage, brain: brain.byo ? `your ${brain.provider} key${brain.model ? ` (${brain.model})` : ""}` : MODEL };

  const lastUserText = messages[messages.length - 1].content;
  scheduleMemoryUpdate(admin, brain, memberId, memory, lastUserText, turn.text);

  if (!turn.plan) return json({ reply: turn.text, ...meta });

  const plan = turn.plan as { license?: { kind?: string; spdx?: string }; title?: string; cards?: unknown[] };
  if (plan?.license?.kind === "open_source" && !plan.license.spdx) plan.license.spdx = "MIT";
  const { data: created, error: cerr } = await admin.rpc("hive_admin_create_project_from_plan", { p_member: memberId, p_plan: plan });
  if (cerr) return json({ reply: turn.text, plan, error: "plan_rejected", detail: cerr.message, ...meta }, { status: 422 });

  return json({
    reply: turn.text || `Created "${plan.title}" with ${plan.cards?.length ?? 0} cards.`,
    plan, project_id: created.project_id, cards: created.cards, ...meta,
  });
});
