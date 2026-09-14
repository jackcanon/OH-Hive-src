import { normalize, providerBody, TurnError, validate } from "./protocol.ts";

type Rpc = (name: string, args: Record<string, unknown>) => PromiseLike<{ data: any; error: unknown }>;
const headers = { "content-type": "application/json", "Access-Control-Allow-Origin": "*", "Access-Control-Allow-Headers": "authorization,x-client-info,apikey,content-type", "Access-Control-Allow-Methods": "POST,OPTIONS" };
const reply = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers });

export function createHandler(rpc: Rpc, transport: typeof fetch = fetch, defaults = { anthropic: "claude-sonnet-4-5", openai: "gpt-5", nous: "anthropic/claude-sonnet-4.6" }) {
  return async (req: Request): Promise<Response> => {
    if (req.method === "OPTIONS") return new Response(null, { status: 204, headers });
    if (req.method !== "POST") return reply({ error: "method_not_allowed" }, 405);
    try {
      // Bound bytes while reading, including chunked requests without Content-Length.
      const reader = req.body?.getReader();
      if (!reader) throw new TurnError(400, "invalid_request");
      const chunks: Uint8Array[] = []; let size = 0;
      for (;;) {
        const { done, value } = await reader.read(); if (done) break;
        size += value.byteLength;
        if (size > 2 * 1024 * 1024) { await reader.cancel(); throw new TurnError(413, "request_too_large"); }
        chunks.push(value);
      }
      const bytes = new Uint8Array(size); let offset = 0;
      for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
      let parsed: unknown;
      try { parsed = JSON.parse(new TextDecoder().decode(bytes)); } catch { throw new TurnError(400, "invalid_json"); }
      const r = validate(parsed);
      const member = await rpc("hive_admin_code_brain_member", { p_raw_key: r.raw_key });
      if (member.error) throw new TurnError(503, "authentication_unavailable");
      if (!member.data) throw new TurnError(401, "invalid_or_revoked_node_key");
      const key = await rpc("hive_admin_member_key", { p_member: member.data, p_provider: r.provider });
      if (key.error) throw new TurnError(503, "key_lookup_unavailable");
      if (typeof key.data !== "string" || !key.data) throw new TurnError(409, "provider_key_not_configured");
      const preferred = await rpc("hive_admin_member_models", { p_member: member.data });
      if (preferred.error) throw new TurnError(503, "model_lookup_unavailable");
      const model = r.model || preferred.data?.[r.provider] || defaults[r.provider];
      const anthropic = r.provider === "anthropic";
      let response: Response;
      try {
        response = await transport(anthropic ? "https://api.anthropic.com/v1/messages" : r.provider === "nous" ? "https://inference-api.nousresearch.com/v1/chat/completions" : "https://api.openai.com/v1/chat/completions", {
          method: "POST", signal: AbortSignal.timeout(90_000),
          headers: anthropic ? { "content-type": "application/json", "x-api-key": key.data, "anthropic-version": "2023-06-01" } : { "content-type": "application/json", authorization: `Bearer ${key.data}` },
          body: JSON.stringify(providerBody(r, model)),
        });
      } catch { throw new TurnError(504, "provider_unreachable_or_timeout"); }
      // Never echo upstream bodies: they can contain private context or credentials.
      if (!response.ok) {
        // Convert known billing failures to a fixed code; discard every upstream string.
        const failure = await response.json().catch(() => null);
        const message = typeof failure?.error?.message === "string" ? failure.error.message.toLowerCase() : "";
        if (message.includes("credit balance") || failure?.error?.code === "insufficient_quota")
          throw new TurnError(402, "provider_credit_or_quota_exhausted");
        if (message.includes("anthropic-workspace-id"))
          throw new TurnError(409, "provider_workspace_scoped_key_required");
        throw new TurnError(response.status === 429 ? 429 : 502, response.status === 401 || response.status === 403 ? "provider_key_rejected" : `provider_request_failed_${response.status}`);
      }
      let output: unknown;
      try { output = await response.json(); } catch { throw new TurnError(502, "invalid_provider_response"); }
      return reply(normalize(r.provider, output, r.tools));
    } catch (e) {
      return reply({ error: e instanceof TurnError ? e.code : "internal_error" }, e instanceof TurnError ? e.status : 500);
    }
  };
}
