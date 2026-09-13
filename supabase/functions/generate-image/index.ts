// Hive — hosted image generation (tasks #125-128, ADR-018 M8 follow-on).
//
// BYOK-only as of 2026-09-13 (Jack, right after seeing the first version of this function:
// "running it hosted will guarantee a spend, I think we've got to make it so that it's bring
// your own key" -- same call he made for chat/interview the day before, see
// 20260912300000_chat_local_and_byok_only.sql and this function's sibling supabase/functions/
// interview/index.ts). This function now ONLY ever uses the requesting member's own OpenAI key,
// from Supabase Vault via hive_admin_member_key (the same BYOK storage the interviewer already
// uses, 20260905000026_interview_provider.sql -- 'openai' was already a supported provider there,
// just for chat until now). A member with no OpenAI key on file gets a clear no_byo_key error and
// no OpenAI call is made; nothing is ever charged to the hub's account, and nothing is charged to
// the member's Honey wallet either, since OpenAI bills the member's own account directly. The
// Local (ComfyUI) path in the Swift app remains the free, no-OpenAI-account-needed alternative.
//
// The original version of this function (2026-09-12) called OpenAI with a hub-wide OPENAI_API_KEY
// secret and charged the member a flat Honey fee via hive_admin_charge_media -- that guaranteed
// real OpenAI spend on Jack's own account for every hosted generation, which is exactly what this
// rewrite removes. hive_admin_charge_media and the hub's OPENAI_API_KEY secret are left in place
// (harmless if unused) in case a hub-funded path is ever wanted again, same spirit as interview/
// index.ts keeping its dead ANTHROPIC_API_KEY note.
//
// POST /generate-image   { raw_key: string, prompt: string, negative_prompt?: string }
// → { image_base64: string }
//
// Node-key authenticated (no member Supabase session on the desktop app) -- verify_node_key +
// node_member resolve the raw node key to the owning member server-side, same pattern as
// submit_feature_request (crates/ohhive-ffi/src/feedback.rs).

import { createClient } from "npm:@supabase/supabase-js@2";

const MODEL = Deno.env.get("GENERATE_IMAGE_MODEL") ?? "gpt-image-2";
const SIZE = Deno.env.get("GENERATE_IMAGE_SIZE") ?? "1024x1024";

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
  const prompt = negative ? `${promptIn}\n\nAvoid: ${negative}` : promptIn;

  const url = Deno.env.get("SUPABASE_URL")!;
  const admin = createClient(url, Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!);

  const { data: nodeId, error: nerr } = await admin.rpc("hive_admin_verify_node_key", { p_raw_key: rawKey });
  if (nerr) return json({ error: "verify_failed", detail: nerr.message }, { status: 500 });
  if (!nodeId) return json({ error: "invalid_or_revoked_node_key" }, { status: 401 });

  const { data: memberId, error: merr } = await admin.rpc("hive_admin_node_member", { p_node_id: nodeId });
  if (merr) return json({ error: "member_lookup_failed", detail: merr.message }, { status: 500 });
  if (!memberId) return json({ error: "no_member_for_node" }, { status: 403 });

  const { data: apiKey, error: kerr } = await admin.rpc("hive_admin_member_key", { p_member: memberId, p_provider: "openai" });
  if (kerr) console.error("hive_admin_member_key(openai) failed:", kerr);
  if (!apiKey) {
    return json(
      { error: "no_byo_key", detail: "add your OpenAI key in Settings to generate images this way, or use Local (ComfyUI) instead" },
      { status: 503 },
    );
  }

  const res = await fetch("https://api.openai.com/v1/images/generations", {
    method: "POST",
    headers: { "content-type": "application/json", authorization: `Bearer ${apiKey}` },
    body: JSON.stringify({ model: MODEL, prompt, size: SIZE, n: 1 }),
  });
  if (!res.ok) {
    const detail = await res.text();
    return json({ error: "provider_error", detail }, { status: 502 });
  }
  const out = await res.json();
  let b64: string | undefined = out.data?.[0]?.b64_json;
  if (!b64 && out.data?.[0]?.url) {
    const imgRes = await fetch(out.data[0].url);
    if (imgRes.ok) {
      const buf = new Uint8Array(await imgRes.arrayBuffer());
      b64 = btoa(String.fromCharCode(...buf));
    }
  }
  if (!b64) return json({ error: "no_image_returned" }, { status: 502 });

  return json({ image_base64: b64 });
});
