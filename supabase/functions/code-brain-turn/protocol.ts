// Wire contract shared with coder.rs. This module never executes tools.
export type Call = { id: string; name: string; arguments: Record<string, unknown> };
export type Message = { role: "system" | "user" | "assistant" | "tool"; content?: string | null; tool_calls?: Call[]; tool_call_id?: string | null };
export type Tool = { name: string; description: string; parameters: Record<string, unknown> };
export type RequestBody = { raw_key: string; provider: "anthropic" | "openai" | "nous"; model?: string | null; messages: Message[]; tools: Tool[] };
export class TurnError extends Error {
  constructor(public status: number, public code: string) { super(code); }
}
const object = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);
const nonempty = (v: unknown): v is string => typeof v === "string" && v.trim().length > 0;
function requireValid(ok: unknown): asserts ok { if (!ok) throw new TurnError(400, "invalid_request"); }

export function validate(input: unknown): RequestBody {
  requireValid(object(input));
  requireValid(nonempty(input.raw_key) && input.raw_key.length <= 1024);
  requireValid(input.provider === "anthropic" || input.provider === "openai" || input.provider === "nous");
  requireValid(input.model == null || (nonempty(input.model) && input.model.length <= 200));
  requireValid(Array.isArray(input.tools) && input.tools.length <= 32);
  const names = new Set<string>();
  for (const t of input.tools) {
    requireValid(object(t) && typeof t.name === "string" && /^[a-zA-Z0-9_-]{1,64}$/.test(t.name));
    requireValid(!names.has(t.name) && typeof t.description === "string" && object(t.parameters));
    names.add(t.name);
  }
  requireValid(Array.isArray(input.messages) && input.messages.length > 0 && input.messages.length <= 1000);
  const pending = new Set<string>(); const used = new Set<string>();
  let started = false;
  for (const m of input.messages) {
    requireValid(object(m) && ["system", "user", "assistant", "tool"].includes(m.role as string));
    requireValid(m.content == null || typeof m.content === "string");
    requireValid(m.tool_calls === undefined || Array.isArray(m.tool_calls));
    const calls = (m.tool_calls ?? []) as unknown[];
    if (m.role === "tool") {
      requireValid(nonempty(m.tool_call_id) && pending.has(m.tool_call_id) && typeof m.content === "string" && calls.length === 0);
      pending.delete(m.tool_call_id);
    } else {
      requireValid(pending.size === 0 && m.tool_call_id == null);
      if (m.role === "system") requireValid(!started && typeof m.content === "string" && calls.length === 0);
      else started = true;
      requireValid(m.role === "assistant" || calls.length === 0);
      requireValid(typeof m.content === "string" || calls.length > 0);
      for (const c of calls) {
        requireValid(object(c) && nonempty(c.id) && !used.has(c.id) && typeof c.name === "string" && names.has(c.name) && object(c.arguments));
        used.add(c.id); pending.add(c.id);
      }
    }
  }
  requireValid(started && pending.size === 0 && ["user", "tool"].includes(input.messages.at(-1).role));
  return input as unknown as RequestBody;
}

export function providerBody(r: RequestBody, model: string): Record<string, unknown> {
  if (r.provider !== "anthropic") return {
    model, ...(r.provider === "nous" ? { max_tokens: 4096 } : { max_completion_tokens: 4096 }),
    messages: r.messages.map(m => ({ role: m.role, content: m.content ?? null,
      ...(m.tool_call_id ? { tool_call_id: m.tool_call_id } : {}),
      ...(m.tool_calls?.length ? { tool_calls: m.tool_calls.map(c => ({ id: c.id, type: "function", function: { name: c.name, arguments: JSON.stringify(c.arguments) } })) } : {}) })),
    ...(r.tools.length ? { tools: r.tools.map(t => ({ type: "function", function: t })) } : {}),
  };
  const messages: { role: string; content: unknown[] }[] = [];
  for (let i = 0; i < r.messages.length; i++) {
    const m = r.messages[i];
    if (m.role === "system") continue;
    if (m.role === "tool") {
      const content: unknown[] = [];
      // Anthropic requires all results for a parallel tool turn in ONE user message.
      while (i < r.messages.length && r.messages[i].role === "tool") {
        const t = r.messages[i++];
        content.push({ type: "tool_result", tool_use_id: t.tool_call_id, content: t.content });
      }
      i--; messages.push({ role: "user", content });
    } else {
      const content: unknown[] = m.content ? [{ type: "text", text: m.content }] : [];
      for (const c of m.tool_calls ?? []) content.push({ type: "tool_use", id: c.id, name: c.name, input: c.arguments });
      messages.push({ role: m.role, content });
    }
  }
  return { model, max_tokens: 4096, system: r.messages.filter(m => m.role === "system").map(m => m.content).join("\n\n"), messages,
    ...(r.tools.length ? { tools: r.tools.map(t => ({ name: t.name, description: t.description, input_schema: t.parameters })) } : {}) };
}

export function normalize(provider: RequestBody["provider"], out: any, tools: Tool[]) {
  const bad = () => { throw new TurnError(502, "invalid_provider_response"); };
  let calls: Call[] = []; let text = ""; let tokens_in = 0; let tokens_out = 0;
  if (provider === "anthropic") {
    if (!["end_turn", "tool_use"].includes(out?.stop_reason) || !Array.isArray(out.content)) return bad();
    calls = out.content.filter((c: any) => c.type === "tool_use").map((c: any) => ({ id: c.id, name: c.name, arguments: c.input }));
    text = out.content.filter((c: any) => c.type === "text").map((c: any) => c.text).join("\n");
    tokens_in = (out.usage?.input_tokens ?? 0) + (out.usage?.cache_creation_input_tokens ?? 0) + (out.usage?.cache_read_input_tokens ?? 0);
    tokens_out = out.usage?.output_tokens ?? 0;
    if ((out.stop_reason === "tool_use") !== (calls.length > 0)) return bad();
  } else {
    const choice = out?.choices?.[0];
    if (!["stop", "tool_calls"].includes(choice?.finish_reason)) return bad();
    try { calls = (choice.message.tool_calls ?? []).map((c: any) => {
      if (c.type !== "function") return bad();
      return { id: c.id, name: c.function.name, arguments: JSON.parse(c.function.arguments) };
    }); } catch { return bad(); }
    text = choice.message.content ?? "";
    tokens_in = out.usage?.prompt_tokens ?? 0; tokens_out = out.usage?.completion_tokens ?? 0;
    if ((choice.finish_reason === "tool_calls") !== (calls.length > 0)) return bad();
  }
  const ids = new Set<string>();
  for (const c of calls) {
    if (!nonempty(c.id) || ids.has(c.id) || !tools.some(t => t.name === c.name) || !object(c.arguments)) return bad();
    ids.add(c.id);
  }
  if (![tokens_in, tokens_out].every(n => Number.isSafeInteger(n) && n >= 0) || typeof text !== "string" || (!calls.length && !text.trim())) return bad();
  return calls.length ? { type: "tool_calls", calls, tokens_in, tokens_out } : { type: "text", text, tokens_in, tokens_out };
}
