import { normalize, providerBody, validate } from "./protocol.ts";
import { createHandler } from "./handler.ts";
const assert = (v: unknown, m = "assertion failed") => { if (!v) throw new Error(m); };
const tools = ["read_file", "list_dir"].map(name => ({ name, description: name, parameters: { type: "object" } }));
const calls = tools.map((t, i) => ({ id: `call-${i}`, name: t.name, arguments: { path: "." } }));
const body = (provider: "anthropic" | "openai" | "nous" = "anthropic") => ({ raw_key: "test-node-key", provider, tools, messages: [
  { role: "system", content: "Help with code" }, { role: "user", content: "Inspect files" },
  { role: "assistant", tool_calls: calls }, { role: "tool", tool_call_id: "call-0", content: "file content" },
  { role: "tool", tool_call_id: "call-1", content: "directory content" },
] });
Deno.test("Anthropic groups two results and retains system and tool inputs", () => {
  const b = providerBody(validate(body()), "model") as any;
  assert(b.system === "Help with code"); assert(b.messages.length === 3);
  assert(b.messages[2].content.length === 2); assert(b.messages[2].content[1].tool_use_id === "call-1");
  assert(b.messages[1].content[0].input.path === ".");
});
Deno.test("OpenAI serializes arguments and preserves both tool result IDs", () => {
  const b = providerBody(validate(body("openai")), "model") as any;
  assert(JSON.parse(b.messages[2].tool_calls[1].function.arguments).path === ".");
  assert(b.messages[3].tool_call_id === "call-0" && b.messages[4].tool_call_id === "call-1");
});
Deno.test("conversation rejects missing, duplicate, orphan and interleaved results", () => {
  const invalid = [body(), body(), body(), body()];
  invalid[0].messages.pop(); invalid[1].messages[4].tool_call_id = "call-0";
  invalid[2].messages[3].tool_call_id = "unknown"; invalid[3].messages[3] = { role: "user", content: "interrupt" };
  for (const b of invalid) { let failed = false; try { validate(b); } catch { failed = true; } assert(failed); }
});
Deno.test("both providers normalize multi-call responses and usage", () => {
  const a = normalize("anthropic", { stop_reason: "tool_use", content: calls.map(c => ({ type: "tool_use", ...c, input: c.arguments })), usage: { input_tokens: 5, output_tokens: 7 } }, tools);
  const o = normalize("openai", { choices: [{ finish_reason: "tool_calls", message: { tool_calls: calls.map(c => ({ id: c.id, type: "function", function: { name: c.name, arguments: JSON.stringify(c.arguments) } })) } }], usage: { prompt_tokens: 5, completion_tokens: 7 } }, tools);
  assert(JSON.stringify(a) === JSON.stringify(o));
});
Deno.test("truncation, malformed arguments and unknown tools fail closed", () => {
  for (const [reason, name, args] of [["length", "read_file", "{}"], ["tool_calls", "read_file", "{"], ["tool_calls", "untrusted", "{}"], ["tool_calls", "read_file", "null"]]) {
    let failed = false;
    try { normalize("openai", { choices: [{ finish_reason: reason, message: { tool_calls: [{ id: "a", type: "function", function: { name, arguments: args } }] } }] }, tools); } catch { failed = true; }
    assert(failed);
  }
});
const request = (b: unknown = body()) => new Request("https://hive.test", { method: "POST", body: JSON.stringify(b) });
Deno.test("invalid node and missing key never reach provider", async () => {
  for (const missing of ["member", "key"]) {
    let fetched = false;
    const handler = createHandler(async name => ({ error: null, data: name === "hive_admin_code_brain_member" ? (missing === "member" ? null : "member") : null }), async () => { fetched = true; return new Response(); });
    const res = await handler(request()); assert(res.status === (missing === "member" ? 401 : 409)); assert(!fetched);
  }
});
Deno.test("handler uses saved model, makes one request, and exposes no secrets", async () => {
  let count = 0;
  const rpc = async (name: string) => ({ error: null, data: name === "hive_admin_code_brain_member" ? "member" : name === "hive_admin_member_key" ? "secret-key" : { anthropic: "saved-model" } });
  const handler = createHandler(rpc, async (_url, init) => {
    count++; assert(JSON.parse(init?.body as string).model === "saved-model");
    return Response.json({ stop_reason: "end_turn", content: [{ type: "text", text: "Done" }], usage: { input_tokens: 1, output_tokens: 2 } });
  });
  const res = await handler(request()); assert(res.status === 200); assert(count === 1); assert((await res.json()).text === "Done");
  const failed = createHandler(rpc, async () => new Response("secret-key private-context", { status: 401 }));
  const error = await failed(request()); assert(error.status === 502); assert(await error.text() === '{"error":"provider_key_rejected"}');
});
// Cmd Work 90baee32: this path was unmetered and uncapped. These three assert the ordering the
// migration's header note commits to -- refuse before spending, book after, and never fail a
// paid-for turn over the bookkeeping.
Deno.test("a member at their monthly ceiling is refused before the provider is called", async () => {
  let fetched = false;
  const handler = createHandler(
    async name => name === "hive_admin_code_brain_guard"
      ? { error: { message: 'code_brain_month_cap_reached: 25.0 of 25 USD used this month' }, data: null }
      : { error: null, data: name === "hive_admin_code_brain_member" ? "member" : name === "hive_admin_member_key" ? "secret-key" : {} },
    async () => { fetched = true; return new Response(); });
  const res = await handler(request());
  assert(res.status === 402); assert(await res.text() === '{"error":"code_brain_month_cap_reached"}');
  assert(!fetched, "the ceiling must be checked before any money is spent");
  // An unavailable guard fails closed too: a spend guard that fails open is not a guard.
  const broken = createHandler(
    async name => name === "hive_admin_code_brain_guard" ? { error: { message: "connection reset" }, data: null }
      : { error: null, data: name === "hive_admin_code_brain_member" ? "member" : name === "hive_admin_member_key" ? "secret-key" : {} },
    async () => { fetched = true; return new Response(); });
  assert((await broken(request())).status === 503); assert(!fetched);
});
Deno.test("a successful turn books its tokens at the configured price", async () => {
  let booked: Record<string, unknown> | null = null;
  const rpc = async (name: string, args: Record<string, unknown>) => {
    if (name === "hive_admin_code_brain_record") { booked = args; return { error: null, data: { turn_usd: 0.000045, spent_usd: 0.000045, remaining_usd: 24.99 } }; }
    return { error: null, data: name === "hive_admin_code_brain_member" ? "member" : name === "hive_admin_member_key" ? "secret-key" : name === "hive_admin_member_models" ? { anthropic: "saved-model" } : {} };
  };
  const handler = createHandler(rpc, async () =>
    Response.json({ stop_reason: "end_turn", content: [{ type: "text", text: "Done" }], usage: { input_tokens: 10, output_tokens: 2 } }));
  const priced = await (await handler(request())).json();
  assert(priced.usage_recorded === true && priced.usage_priced === true);
  assert(priced.spend.remaining_usd === 24.99, "the caller is told what is left, not just that it worked");
  assert(booked!.p_tokens_in === 10 && booked!.p_tokens_out === 2, "the counts normalize() already parsed");
  assert(booked!.p_model === "saved-model", "priced against the model actually used, not the request's preference");
  assert(booked!.p_usd_in_per_m === 3.0 && booked!.p_usd_out_per_m === 15.0);
  // An unpriced provider books real tokens at no dollar estimate rather than a guessed one.
  const unpriced = createHandler(rpc, async () =>
    Response.json({ choices: [{ finish_reason: "stop", message: { content: "Done" } }], usage: { prompt_tokens: 7, completion_tokens: 1 } }));
  const nous = await (await unpriced(request(body("nous")))).json();
  assert(nous.usage_priced === false && nous.usage_recorded === true);
  assert(booked!.p_usd_in_per_m === 0 && booked!.p_tokens_in === 7);
});
Deno.test("a failed booking does not throw away a turn the member already paid for", async () => {
  const handler = createHandler(
    async name => name === "hive_admin_code_brain_record" ? { error: { message: "deadlock detected" }, data: null }
      : { error: null, data: name === "hive_admin_code_brain_member" ? "member" : name === "hive_admin_member_key" ? "secret-key" : {} },
    async () => Response.json({ stop_reason: "end_turn", content: [{ type: "text", text: "Done" }], usage: { input_tokens: 1, output_tokens: 1 } }));
  const res = await handler(request()); const body = await res.json();
  assert(res.status === 200 && body.text === "Done", "the answer is returned; the money is already gone either way");
  assert(body.usage_recorded === false, "but the caller is told the meter missed, not left to trust a short total");
});
Deno.test("malformed JSON and oversized requests fail before authentication", async () => {
  const handler = createHandler(async () => { throw new Error("must not authenticate"); });
  assert((await handler(new Request("https://hive.test", { method: "POST", body: "{" }))).status === 400);
  assert((await handler(new Request("https://hive.test", { method: "POST", body: "x".repeat(2097153) }))).status === 413);
});
Deno.test("Nous uses Portal endpoint, saved model and OpenAI tool protocol", async () => {
  let count = 0;
  const rpc = async (name: string) => ({ error: null, data: name === "hive_admin_code_brain_member" ? "member" : name === "hive_admin_member_key" ? "synthetic-key" : { nous: "anthropic/claude-sonnet-4.6" } });
  const handler = createHandler(rpc, async (url, init) => {
    count++; assert(url === "https://inference-api.nousresearch.com/v1/chat/completions");
    const b = JSON.parse(init?.body as string); assert(b.model === "anthropic/claude-sonnet-4.6" && b.max_tokens === 4096);
    assert(b.messages[4].tool_call_id === "call-1");
    return Response.json({ choices: [{ finish_reason: "stop", message: { content: "Done" } }], usage: { prompt_tokens: 5, completion_tokens: 2 } });
  });
  const r = await handler(request({ ...body(), provider: "nous" }));
  assert(r.status === 200 && (await r.json()).text === "Done" && count === 1);
});
Deno.test("workspace and billing errors expose fixed actionable codes only", async () => {
  const rpc = async (name: string) => ({ error: null, data: name === "hive_admin_code_brain_member" ? "member" : name === "hive_admin_member_key" ? "secret-key" : {} });
  for (const [message, code] of [["Use anthropic-workspace-id secret-key", "provider_workspace_scoped_key_required"], ["Your credit balance is too low secret-key", "provider_credit_or_quota_exhausted"]]) {
    const handler = createHandler(rpc, async () => Response.json({ error: { message } }, { status: 400 }));
    const r = await handler(request()); assert((await r.json()).error === code);
  }
});
Deno.test("Rust hub serializes absent model as null", () => {
  const r = validate({ ...body(), model: null }); assert(r.model === null);
});
