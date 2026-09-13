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
//
// POST /interview   Authorization: Bearer <member's Supabase JWT>
//   { messages: [{role:'user'|'assistant', content:string}], mode?: 'chat'|'plan' }
// → { reply: string, plan?: ProjectPlan, project_id?: string, charged: number, balance: number }
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

// mode: "chat" -- no plan-steering, no tool offered. Just a normal, helpful conversation; if the
// member wants to build something on Hive they'll say so themselves via the "Turn this into a
// project" button, which switches subsequent turns to planSystemPrompt below.
function chatSystemPrompt(memberName: string, webSearch: boolean) {
  return `You are Hive's assistant, talking with ${memberName}. This is a normal conversation — answer questions, help them think something through, write or edit something, explain code, whatever they're after. You're not gathering requirements for anything and there's no hidden agenda.

Hive is an invite-only community compute network where members can also turn a conversation into a project — a kanban of cards that idle member machines run — but that only happens if ${memberName} asks for it or clicks the button for it. Don't steer toward that, don't ask the questions you'd ask to scope a project (audience, license, internet access, etc.), and don't bring up "cards," "the plan," or "the coordinator" unless they do first.${webSearch ? " You can search the web when it would help answer something." : ""}`;
}

// mode: "plan" -- the original interview behavior.
function planSystemPrompt(capacity: unknown, memberName: string, webSearch: boolean) {
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

When you call the tool, also write a one-paragraph reply summarising the plan for the member in plain language. Never mention "the interview," "the plan," "cards," "coordinator," or any other Hive-internal mechanics unless the member brings them up first — from where they're sitting, they just described something and it's getting made.`;
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

async function callAnthropic(apiKey: string, system: string, messages: Msg[], webSearch: boolean, includePlanTool: boolean): Promise<Turn> {
  const tools: unknown[] = [];
  if (includePlanTool) tools.push({ name: "create_project_plan", description: "Create the project and its kanban cards. Call once, when you have enough to.", input_schema: PLAN_SCHEMA });
  if (webSearch) tools.push({ type: "web_search_20250305", name: "web_search", max_uses: 3 });
  const body: Record<string, unknown> = { model: MODEL, max_tokens: 2500, system, messages };
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

function callOpenAI(apiKey: string, system: string, messages: Msg[], includePlanTool: boolean): Promise<Turn> {
  return callOpenAICompatible("https://api.openai.com/v1", OPENAI_MODEL, apiKey, system, messages, "openai", includePlanTool);
}

function callNous(apiKey: string, system: string, messages: Msg[], includePlanTool: boolean): Promise<Turn> {
  return callOpenAICompatible(NOUS_BASE_URL, NOUS_MODEL, apiKey, system, messages, "nous", includePlanTool);
}

Deno.serve(async (req) => {
  if (req.method === "OPTIONS") return new Response("ok", { headers: corsHeaders });
  if (req.method !== "POST") return new Response("POST only", { status: 405, headers: corsHeaders });

  const auth = req.headers.get("Authorization") ?? "";
  const url = Deno.env.get("SUPABASE_URL")!;
  const userClient = createClient(url, Deno.env.get("SUPABASE_ANON_KEY")!, { global: { headers: { Authorization: auth } } });
  const { data: { user }, error: uerr } = await userClient.auth.getUser();
  if (uerr || !user) return json({ error: "unauthenticated" }, { status: 401 });

  const admin = createClient(url, Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!);
  const { data: active } = await admin.rpc("hive_admin_member_active", { p_member: user.id });
  if (!active) return json({ error: "not_a_hive_member" }, { status: 403 });

  const body = await req.json().catch(() => ({}));
  const messages: Msg[] = Array.isArray(body.messages) ? body.messages : [];
  if (messages.length === 0 || messages[messages.length - 1].role !== "user") {
    return json({ error: "messages must end with a user turn" }, { status: 400 });
  }
  const mode: "chat" | "plan" = body.mode === "plan" ? "plan" : "chat";
  const includePlanTool = mode === "plan";

  // Which brain: the member's own key first (free to the Hive), then the hub's.
  const [anthropicRes, openaiRes, nousRes, cfgRes] = await Promise.all([
    admin.rpc("hive_admin_member_key", { p_member: user.id, p_provider: "anthropic" }),
    admin.rpc("hive_admin_member_key", { p_member: user.id, p_provider: "openai" }),
    admin.rpc("hive_admin_member_key", { p_member: user.id, p_provider: "nous" }),
    admin.rpc("hive_admin_setting", { p_key: "interview_web_search" }),
  ]);
  // These RPCs fail closed (silently, as far as the member sees) on a permission or query error --
  // log so a misconfigured grant shows up in function_logs instead of masquerading as "no key set".
  if (anthropicRes.error) console.error("hive_admin_member_key(anthropic) failed:", anthropicRes.error);
  if (openaiRes.error) console.error("hive_admin_member_key(openai) failed:", openaiRes.error);
  if (nousRes.error) console.error("hive_admin_member_key(nous) failed:", nousRes.error);
  if (cfgRes.error) console.error("hive_admin_setting(interview_web_search) failed:", cfgRes.error);
  const byoAnthropic = anthropicRes.data;
  const byoOpenAI = openaiRes.data;
  const byoNous = nousRes.data;
  const cfg = cfgRes.data;
  const webSearch = cfg !== false && cfg !== "false";
  type Brain = { provider: "anthropic" | "openai" | "nous"; key: string; byo: boolean };
  // Try every configured BYO key in priority order rather than committing to the first one found --
  // a single bad key (e.g. an unscoped Anthropic key) shouldn't block the turn when another usable
  // key is on file. No hub fallback (2026-09-12): every "byo" here is always true.
  const candidates: Brain[] = [
    byoAnthropic && { provider: "anthropic" as const, key: byoAnthropic, byo: true },
    byoOpenAI && { provider: "openai" as const, key: byoOpenAI, byo: true },
    byoNous && { provider: "nous" as const, key: byoNous, byo: true },
  ].filter((b): b is Brain => Boolean(b));
  if (candidates.length === 0) return json({ error: "no_byo_key", detail: "no API key on file — add one in Settings, or use local chat instead" }, { status: 503 });

  const { data: capacity } = await admin.rpc("hive_capacity_summary");
  const { data: prof } = await admin.from("profiles").select("display_name").eq("id", user.id).maybeSingle();

  let brain: Brain | null = null;
  let turn: Turn | null = null;
  let lastError: unknown = null;
  for (const candidate of candidates) {
    const anthropicWebSearch = candidate.provider === "anthropic" && webSearch;
    const system = mode === "chat"
      ? chatSystemPrompt(prof?.display_name ?? "the member", anthropicWebSearch)
      : planSystemPrompt(capacity, prof?.display_name ?? "the member", anthropicWebSearch);
    try {
      turn = candidate.provider === "anthropic" ? await callAnthropic(candidate.key, system, messages, webSearch, includePlanTool)
        : candidate.provider === "openai" ? await callOpenAI(candidate.key, system, messages, includePlanTool)
        : await callNous(candidate.key, system, messages, includePlanTool);
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
      p_member: user.id, p_tokens_in: turn.tokens_in, p_tokens_out: turn.tokens_out,
      p_usd_in_per_m: PRICE.in, p_usd_out_per_m: PRICE.out + (turn.tokens_out > 0 ? (searchUsd * 1e6) / turn.tokens_out : 0),
      p_memo: `${mode} turn (${MODEL}${turn.web_searches ? `, ${turn.web_searches} web search${turn.web_searches === 1 ? "" : "es"}` : ""})`,
    });
    charge = data;
  }
  const usage = { tokens_in: turn.tokens_in, tokens_out: turn.tokens_out, web_searches: turn.web_searches };
  const meta = { charged: charge?.charged ?? 0, balance: charge?.balance ?? null, usage, brain: brain.byo ? `your ${brain.provider} key` : MODEL };

  if (!turn.plan) return json({ reply: turn.text, ...meta });

  const plan = turn.plan as { license?: { kind?: string; spdx?: string }; title?: string; cards?: unknown[] };
  if (plan?.license?.kind === "open_source" && !plan.license.spdx) plan.license.spdx = "MIT";
  const { data: created, error: cerr } = await admin.rpc("hive_admin_create_project_from_plan", { p_member: user.id, p_plan: plan });
  if (cerr) return json({ reply: turn.text, plan, error: "plan_rejected", detail: cerr.message, ...meta }, { status: 422 });

  return json({
    reply: turn.text || `Created "${plan.title}" with ${plan.cards?.length ?? 0} cards.`,
    plan, project_id: created.project_id, cards: created.cards, ...meta,
  });
});
