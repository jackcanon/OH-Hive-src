import { createHandler, type Dependencies, validate } from "./handler.ts";
const owner = "12345678-1234-1234-1234-123456789abc";
const turn = {
  owner_id: owner,
  provider: "nous",
  agent_name: "Nous",
  participants_note: "Team",
  messages: [{ speaker: "Jack", text: "Hello" }],
  raw_key: "node-secret",
};
function assert(ok: unknown) {
  if (!ok) throw new Error("Assertion failed");
}
function setup(
  status = 200,
  output: unknown = {
    choices: [{ message: { content: "Hello Jack" } }],
    usage: { prompt_tokens: 4, completion_tokens: 2 },
  },
) {
  let calls = 0;
  let keyLookups = 0;
  const deps: Dependencies = {
    authenticate: async (_auth, raw) => raw === "node-secret" ? owner : null,
    credentials: async (member, provider) => {
      assert(member === owner && ["nous", "anthropic"].includes(provider));
      keyLookups++;
      return { key: "provider-secret", model: "saved-model" };
    },
    fetch: async (input, init) => {
      calls++;
      assert(String(input).startsWith("https://"));
      const body = JSON.parse(String(init?.body));
      assert(
        body.max_tokens === 2048 && !body.tools && body.model === "saved-model",
      );
      assert(!String(init?.body).includes("node-secret"));
      return new Response(JSON.stringify(output), { status });
    },
  };
  const request = (body: unknown = turn) =>
    new Request("https://hub.test", {
      method: "POST",
      body: JSON.stringify(body),
    });
  return {
    deps,
    request,
    handle: (body: unknown = turn) => createHandler(deps)(request(body)),
    calls: () => calls,
    keys: () => keyLookups,
  };
}
Deno.test("named bounded turn uses only selected provider and server key", async () => {
  const t = setup();
  const r = await t.handle();
  const body = await r.json();
  assert(
    r.status === 200 && body.reply_body === "Hello Jack" && t.calls() === 1 &&
      body.usage.prompt_tokens === 4,
  );
  assert(!JSON.stringify(body).includes("secret"));
});
Deno.test("authentication and owner mismatch cannot access keys or provider", async () => {
  for (
    const change of [{ raw_key: "revoked" }, {
      owner_id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
    }]
  ) {
    const t = setup();
    const r = await t.handle({ ...turn, ...change });
    assert([401, 403].includes(r.status) && t.keys() === 0 && t.calls() === 0);
  }
});
Deno.test("provider rate limits and errors sanitized without fallback", async () => {
  for (const status of [429, 401, 500]) {
    const t = setup(status, { error: "provider-secret and private data" });
    const r = await t.handle();
    assert(r.status === (status === 429 ? 429 : 502) && t.calls() === 1);
    assert(!(await r.text()).includes("secret"));
  }
});
Deno.test("missing key fails without provider call", async () => {
  const t = setup();
  t.deps.credentials = async () => null;
  assert((await t.handle()).status === 409 && t.calls() === 0);
});
Deno.test("bounds include message count, UTF8 bytes, roster and encoded request", async () => {
  for (
    const change of [
      { messages: Array(66).fill(turn.messages[0]) },
      { messages: [{ speaker: "Jack", text: "é".repeat(32769) }] },
      { participants_note: "x".repeat(8193) },
      { provider: "openai" },
      { agent_name: "" },
    ]
  ) {
    const t = setup();
    assert(
      (await t.handle({ ...turn, ...change })).status === 400 &&
        t.calls() === 0,
    );
  }
  const t = setup();
  assert(
    (await t.handle({ ...turn, extra: "x".repeat(131073) })).status === 413,
  );
  validate(turn);
});
Deno.test("bad and oversized replies fail closed", async () => {
  for (const content of ["", "x".repeat(65537), { invalid: true }]) {
    const t = setup(200, { choices: [{ message: { content } }] });
    assert((await t.handle()).status === 502);
  }
});
Deno.test("Anthropic shape and usage", async () => {
  const t = setup(200, {
    content: [{ type: "text", text: "Hi" }],
    usage: { input_tokens: 3, output_tokens: 1 },
  });
  const r = await t.handle({ ...turn, provider: "anthropic" });
  assert(r.status === 200 && (await r.json()).usage.prompt_tokens === 3);
});
Deno.test("network timeout and cancellation are sanitized", async () => {
  const t = setup();
  t.deps.fetch = async () => {
    throw new DOMException("secret", "AbortError");
  };
  const r = await t.handle();
  assert(r.status === 502 && !(await r.text()).includes("secret"));
});
