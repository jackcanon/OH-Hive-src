// Hive — hosted image generation (M8 follow-on, tasks #125-128). Jack: "start with the media
// generation backlog" -- ComfyUI (crates/ohhive-core/src/backend/comfyui.rs) already lets a member
// generate images through their OWN configured server; this is the other half -- a default,
// zero-setup path through OpenAI's image API, paid for out of the member's Honey.
//
// Node-key authenticated, NOT member-JWT authenticated. Every other member-facing Edge Function
// here (`interview`) reads `Authorization: Bearer <member JWT>` because the web app always has a
// browser session. The desktop/CLI app calling this one has no such session -- only a long-lived
// node key (hive.node_keys), the same credential node_checkin/node_complete_card already trust.
// 20260912190000_hosted_media_generation.sql's two `hive_admin_*` wrappers exist for exactly this:
// resolve a raw node key -> node_id -> owning member, from a service-role caller. Deployed with
// verify_jwt: false since there is no JWT to check; the node key itself is the auth.
//
// POST /generate-image   (no Authorization header expected)
//   { raw_key: string, prompt: string, negative_prompt?: string }
// -> { image_base64: string, charged: number, balance: number | null }
//    | { error: string, detail?: string }
//
// Cost is a flat, known-up-front USD-per-image (unlike per-token chat), so this charges the
// member's Honey BEFORE calling OpenAI, not after (the opposite order from `interview`'s
// charge-after-the-fact). That means a member who can't afford it gets told immediately and the
// hub never spends real OpenAI dollars on a call nobody can pay for -- no refund logic needed
// because a failed charge means OpenAI is simply never called.
//
// PRICE_USD is a placeholder -- verify it against OpenAI's actual current per-image price for
// MODEL/SIZE before this is live for real members; getting it wrong either overcharges people or
// quietly loses the hub money on every image. Both are overridable via secrets without a redeploy.

import { createClient } from "npm:@supabase/supabase-js@2";

const MODEL = Deno.env.get("GENERATE_IMAGE_MODEL") ?? "gpt-image-2";
const SIZE = Deno.env.get("GENERATE_IMAGE_SIZE") ?? "1024x1024";
// USD per image at SIZE. Placeholder -- confirm against OpenAI's current pricing for MODEL/SIZE.
const PRICE_USD = Number(Deno.env.get("GENERATE_IMAGE_PRICE_USD") ?? "0.04");

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "content-type",
  "Access-Control-Allow-Methods": "POST, OPTIONS",
};
function json(body: unknown, init?: ResponseInit) {
  return Response.json(body, { ...init, headers: { ...corsHeaders, ...(init?.headers ?? {}) } });
}

Deno.serve(async (req) => {
  if (req.method === "OPTIONS") return new Response("ok", { headers: corsHeaders });
  if (req.method !== "POST") return new Response("POST only", { status: 405, headers: corsHeaders });

  const body = await req.json().catch(() => ({}));
  const rawKey = typeof body.raw_key === "string" ? body.raw_key : "";
  const promptIn = typeof body.prompt === "string" ? body.prompt.trim() : "";
  const negative = typeof body.negative_prompt === "string" ? body.negative_prompt.trim() : "";
  if (!rawKey) return json({ error: "missing_raw_key" }, { status: 401 });
  if (!promptIn) return json({ error: "missing_prompt" }, { status: 400 });
  // gpt-image has no dedicated negative-prompt field (that's a Stable Diffusion/ComfyUI concept) --
  // fold it into the prompt itself so the UI's existing "negative prompt" field still does something.
  const prompt = negative ? `${promptIn}\n\nAvoid: ${negative}` : promptIn;

  const url = Deno.env.get("SUPABASE_URL")!;
  const admin = createClient(url, Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!);

  const { data: nodeId, error: nerr } = await admin.rpc("hive_admin_verify_node_key", { p_raw_key: rawKey });
  if (nerr) { console.error("hive_admin_verify_node_key failed:", nerr); return json({ error: "verify_failed", detail: nerr.message }, { status: 500 }); }
  if (!nodeId) return json({ error: "invalid_or_revoked_node_key" }, { status: 401 });

  const { data: memberId, error: merr } = await admin.rpc("hive_admin_node_member", { p_node_id: nodeId });
  if (merr) { console.error("hive_admin_node_member failed:", merr); return json({ error: "member_lookup_failed", detail: merr.message }, { status: 500 }); }
  if (!memberId) return json({ error: "no_member_for_node" }, { status: 403 });

  const apiKey = Deno.env.get("OPENAI_API_KEY");
  if (!apiKey) return json({ error: "hub_not_configured", detail: "OPENAI_API_KEY secret is missing" }, { status: 503 });

  // Charge first -- see header. A failed charge (empty wallet, provider budget exhausted) means
  // OpenAI is never called.
  const { data: charge, error: cerr } = await admin.rpc("hive_admin_charge_media", {
    p_member: memberId, p_usd_cost: PRICE_USD, p_entry_type: "spend_job",
    p_memo: `hosted image generation (${MODEL}, ${SIZE})`,
  });
  if (cerr) return json({ error: "charge_failed", detail: cerr.message }, { status: 402 });

  const res = await fetch("https://api.openai.com/v1/images/generations", {
    method: "POST",
    headers: { "content-type": "application/json", authorization: `Bearer ${apiKey}` },
    body: JSON.stringify({ model: MODEL, prompt, size: SIZE, n: 1 }),
  });
  if (!res.ok) {
    const detail = await res.text();
    console.error(`openai image generation failed (charged ${charge?.charged ?? 0} honey already): ${detail}`);
    return json({ error: "provider_error", detail, charged: charge?.charged ?? 0, balance: charge?.balance ?? null }, { status: 502 });
  }
  const out = await res.json();
  let b64: string | undefined = out.data?.[0]?.b64_json;
  if (!b64 && out.data?.[0]?.url) {
    // Some response modes return a hosted URL instead of inline base64 -- fetch it ourselves so
    // the caller always gets image bytes back, never a second URL to chase.
    const imgRes = await fetch(out.data[0].url);
    if (imgRes.ok) {
      const buf = new Uint8Array(await imgRes.arrayBuffer());
      b64 = btoa(String.fromCharCode(...buf));
    }
  }
  if (!b64) return json({ error: "no_image_returned", charged: charge?.charged ?? 0, balance: charge?.balance ?? null }, { status: 502 });

  return json({ image_base64: b64, charged: charge?.charged ?? 0, balance: charge?.balance ?? null });
});
