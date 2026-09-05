// OH Hive — interviewer agent (ADR-006 D37–D39, ADR-011 D54, ADR-002 §7).
//
// POST /interview   Authorization: Bearer <member's Supabase JWT>
//   { messages: [{role:'user'|'assistant', content:string}] }   (the whole conversation so far)
// → { reply: string, plan?: ProjectPlan, project_id?: string, charged: number, balance: number }
//
// One Claude call per turn with a single tool, `create_project_plan`, whose input schema is the
// ProjectPlan contract (packages/schema/project-plan.schema.json). When the model has enough, it
// calls the tool; we validate, materialize projects + cards via hive.create_project_from_plan,
// and charge the member at provider cost through the peg. Provider keys never leave the hub.
//
// Secrets: ANTHROPIC_API_KEY (required), INTERVIEW_MODEL (default claude-sonnet-4-5). Supabase
// injects SUPABASE_URL / SUPABASE_SERVICE_ROLE_KEY / SUPABASE_ANON_KEY.

import { createClient } from "npm:@supabase/supabase-js@2";

const MODEL = Deno.env.get("INTERVIEW_MODEL") ?? "claude-sonnet-4-5";
// USD per million tokens for the interview model — keep in step with the rate table's peg.
const PRICE = { in: 3.0, out: 15.0 };

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

function systemPrompt(capacity: unknown, memberName: string) {
  return `You are the OH Hive interviewer. OH Hive is an invite-only community compute network: members contribute idle computers ("nodes"), earn $honey, and spend it on projects. A project is a kanban of cards; each card is one unit of AI work (text, code, image, video, speech, music) that a node runs.

Your job: interview ${memberName} about the project they want, then call create_project_plan exactly once when — and only when — you know all of:
1. What they want to make, concretely enough to write cards with acceptance criteria.
2. Whether the work needs the internet (web fetch, APIs). Ask explicitly. Most creative work does not.
3. License: owner-only, or open source (then which SPDX id — suggest MIT for code, CC-BY-4.0 for media).

Rules for the plan:
- 2–8 cards. Each card is one deliverable a single model run can produce. Give every card a stable lowercase key, a task written as an instruction to the worker, and acceptance criteria a reviewer can check.
- Use deps to order cards (a card's inputs may reference upstream keys by name).
- Prefer modalities the Hive can run today. Current capacity: ${JSON.stringify(capacity)}. If they need a modality with zero nodes, say so and still plan it (it will queue).
- Don't set required_capabilities.model_id unless the member names a model.
- Keep questions short — one or two per turn. Don't ask what you can infer. Three turns is typical.

When you call the tool, also write a one-paragraph reply summarising the plan for the member.`;
}

Deno.serve(async (req) => {
  if (req.method !== "POST") return new Response("POST only", { status: 405 });
  const apiKey = Deno.env.get("ANTHROPIC_API_KEY");
  if (!apiKey) return Response.json({ error: "hub_not_configured", detail: "ANTHROPIC_API_KEY secret missing" }, { status: 503 });

  const auth = req.headers.get("Authorization") ?? "";
  const url = Deno.env.get("SUPABASE_URL")!;
  const userClient = createClient(url, Deno.env.get("SUPABASE_ANON_KEY")!, { global: { headers: { Authorization: auth } } });
  const { data: { user }, error: uerr } = await userClient.auth.getUser();
  if (uerr || !user) return Response.json({ error: "unauthenticated" }, { status: 401 });

  // Service-role client; goes through public.hive_admin_* wrappers until schema `hive` is exposed.
  const admin = createClient(url, Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!);
  const { data: active } = await admin.rpc("hive_admin_member_active", { p_member: user.id });
  if (!active) return Response.json({ error: "not_a_hive_member" }, { status: 403 });

  const body = await req.json().catch(() => ({}));
  const messages: { role: "user" | "assistant"; content: string }[] = Array.isArray(body.messages) ? body.messages : [];
  if (messages.length === 0 || messages[messages.length - 1].role !== "user") {
    return Response.json({ error: "messages must end with a user turn" }, { status: 400 });
  }

  const { data: capacity } = await admin.rpc("hive_capacity_summary");
  const { data: prof } = await admin.from("profiles").select("display_name").eq("id", user.id).maybeSingle();

  const res = await fetch("https://api.anthropic.com/v1/messages", {
    method: "POST",
    headers: { "content-type": "application/json", "x-api-key": apiKey, "anthropic-version": "2023-06-01" },
    body: JSON.stringify({
      model: MODEL,
      max_tokens: 2000,
      system: systemPrompt(capacity, prof?.display_name ?? "the member"),
      messages,
      tools: [{ name: "create_project_plan", description: "Create the project and its kanban cards. Call once, when the interview is complete.", input_schema: PLAN_SCHEMA }],
    }),
  });
  if (!res.ok) return Response.json({ error: "provider_error", detail: await res.text() }, { status: 502 });
  const out = await res.json();

  const usage = { tokens_in: out.usage?.input_tokens ?? 0, tokens_out: out.usage?.output_tokens ?? 0 };
  const { data: charge } = await admin.rpc("hive_admin_charge_interview", {
    p_member: user.id, p_tokens_in: usage.tokens_in, p_tokens_out: usage.tokens_out,
    p_usd_in_per_m: PRICE.in, p_usd_out_per_m: PRICE.out, p_memo: `interview turn (${MODEL})`,
  });

  const text = (out.content ?? []).filter((c: { type: string }) => c.type === "text").map((c: { text: string }) => c.text).join("\n").trim();
  const tool = (out.content ?? []).find((c: { type: string }) => c.type === "tool_use");

  if (!tool) return Response.json({ reply: text, charged: charge?.charged ?? 0, balance: charge?.balance ?? null, usage });

  const plan = tool.input;
  if (plan?.license?.kind === "open_source" && !plan.license.spdx) plan.license.spdx = "MIT";
  const { data: created, error: cerr } = await admin.rpc("hive_admin_create_project_from_plan", { p_member: user.id, p_plan: plan });
  if (cerr) return Response.json({ reply: text, plan, error: "plan_rejected", detail: cerr.message, charged: charge?.charged ?? 0, balance: charge?.balance ?? null }, { status: 422 });

  return Response.json({
    reply: text || `Created "${plan.title}" with ${plan.cards.length} cards.`,
    plan, project_id: created.project_id, cards: created.cards,
    charged: charge?.charged ?? 0, balance: charge?.balance ?? null, usage,
  });
});
