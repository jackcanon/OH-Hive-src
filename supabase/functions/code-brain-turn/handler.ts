import { normalize, providerBody, type RequestBody, TurnError, validate } from "./protocol.ts";

type Rpc = (name: string, args: Record<string, unknown>) => PromiseLike<{ data: any; error: unknown }>;
const headers = { "content-type": "application/json", "Access-Control-Allow-Origin": "*", "Access-Control-Allow-Headers": "authorization,x-client-info,apikey,content-type", "Access-Control-Allow-Methods": "POST,OPTIONS" };
const reply = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers });

/** USD per million tokens. `models` overrides the provider rate for an exact model id. */
export type Price = { in: number; out: number; models?: Record<string, { in: number; out: number }> };
export type Prices = Partial<Record<RequestBody["provider"], Price>>;

/** A price is only usable if both halves are real non-negative numbers. */
const usable = (p: unknown): p is { in: number; out: number } =>
  !!p && typeof p === "object"
  && Number.isFinite((p as { in: unknown }).in) && (p as { in: number }).in >= 0
  && Number.isFinite((p as { out: unknown }).out) && (p as { out: number }).out >= 0;

/**
 * Resolve the rate for one provider+model. Exact model id first, provider rate second, nothing
 * third -- and "nothing" is a real outcome, not a zero: an unpriced turn records its real token
 * counts with `usage_priced: false` rather than pretending it cost nothing at a guessed rate.
 *
 * Per-model matters because the spread inside one provider is enough to break a ceiling: gpt-5 is
 * $1.25/$10 per Mtok and gpt-5.5 is $5/$30. A provider-wide price set from the cheap model
 * silently under-counts every turn on the expensive one.
 */
export function rateFor(prices: Prices, provider: RequestBody["provider"], model: string) {
  const p = prices[provider];
  if (!p) return undefined;
  const exact = p.models?.[model];
  if (usable(exact)) return { in: exact.in, out: exact.out };
  return usable(p) ? { in: p.in, out: p.out } : undefined;
}

export function createHandler(
  rpc: Rpc, transport: typeof fetch = fetch,
  defaults = { anthropic: "claude-sonnet-4-5", openai: "gpt-5", nous: "anthropic/claude-sonnet-4.6" },
  // Anthropic's pair is the one `supabase/functions/interview/index.ts:78` already prices the
  // interviewer at, reused rather than invented. openai/nous are left out on purpose: a wrong
  // price is worse than a missing one, and index.ts reads them from env when someone supplies
  // real figures.
  PRICES: Prices = { anthropic: { in: 3.0, out: 15.0 } },
) {
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
      // A turn's cost is not knowable until the provider answers, so the ceiling is checked here
      // and the actual spend booked after. That bounds the overshoot to one turn rather than to
      // zero -- see the migration's header note. A member already at their monthly ceiling is
      // refused before any money is spent; a lookup that is merely unavailable does not fail the
      // turn open, because failing open on a spend guard is how you find out it was load-bearing.
      const guard = await rpc("hive_admin_code_brain_guard", { p_member: member.data });
      if (guard.error) {
        const reason = String((guard.error as { message?: string })?.message ?? "");
        if (reason.includes("code_brain_month_cap_reached")) throw new TurnError(402, "code_brain_month_cap_reached");
        throw new TurnError(503, "spend_guard_unavailable");
      }
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
      const turn = normalize(r.provider, output, r.tools);
      // Book the turn. `normalize` has always parsed these counts and nothing has ever stored them.
      //
      // Two deliberate choices. (1) A failed record does NOT fail the turn: the provider call has
      // already happened on the member's own key, so throwing away a paid-for answer to punish a
      // bookkeeping error costs the member twice. The response says `usage_recorded: false` instead,
      // so a caller can see the meter missed rather than trusting a total that is quietly short.
      // (2) An unpriced provider records tokens with usd_estimate 0 and reports
      // `usage_priced: false`. Recording a real token count at a fabricated dollar price would be
      // worse than recording no price at all, and the caller can tell the difference. The
      // consequence is that an unpriced provider does not contribute to the monthly ceiling -- set
      // its price env var to bring it under the cap.
      // Prices come from `hive.settings.code_brain_prices` when present, so repricing is one UPDATE
      // rather than a redeploy -- the monthly ceiling already lives there and these are the other
      // two numbers that decide whether it binds. The compiled-in table is the fallback for a
      // database that predates 20260917030000. A malformed settings value is ignored rather than
      // trusted: `rateFor` requires both halves to be finite and non-negative.
      const configured = await rpc("hive_admin_setting", { p_key: "code_brain_prices" })
        .then(res => (res.error || !res.data || typeof res.data !== "object") ? null : res.data as Prices,
              () => null);
      const price = rateFor(configured ?? PRICES, r.provider, model)
        ?? (configured ? rateFor(PRICES, r.provider, model) : undefined);
      const recorded = await rpc("hive_admin_code_brain_record", {
        p_member: member.data, p_provider: r.provider, p_model: model,
        p_tokens_in: turn.tokens_in, p_tokens_out: turn.tokens_out,
        p_usd_in_per_m: price?.in ?? 0, p_usd_out_per_m: price?.out ?? 0,
      }).then(res => res.error ? null : res.data, () => null);
      return reply({ ...turn, usage_recorded: recorded !== null, usage_priced: !!price, spend: recorded ?? null });
    } catch (e) {
      return reply({ error: e instanceof TurnError ? e.code : "internal_error" }, e instanceof TurnError ? e.status : 500);
    }
  };
}
