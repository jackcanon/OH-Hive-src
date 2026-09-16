import { createClient } from "npm:@supabase/supabase-js@2";
import { createHandler } from "./handler.ts";
const admin = createClient(
  Deno.env.get("SUPABASE_URL")!,
  Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!,
  {
    auth: { persistSession: false, autoRefreshToken: false },
    global: {
      fetch: (input, init) =>
        fetch(input, { ...init, signal: AbortSignal.timeout(10_000) }),
    },
  },
);
async function rpc(name: string, args: Record<string, unknown>) {
  const { data, error } = await admin.rpc(name, args);
  if (error) throw new Error("Authentication or key lookup failed");
  return data;
}
Deno.serve(createHandler({
  authenticate: async (authorization, rawKey) => {
    let member: string | null = null;
    // Explicit node credentials must validate; never fall back to another identity.
    if (rawKey) {
      const node = await rpc("hive_admin_verify_node_key", {
        p_raw_key: rawKey,
      });
      if (!node) return null;
      member = await rpc("hive_admin_node_member", { p_node_id: node });
    } else if (authorization.startsWith("Bearer ")) {
      const { data, error } = await admin.auth.getUser(authorization.slice(7));
      if (!error) member = data.user?.id ?? null;
    }
    return member &&
        await rpc("hive_admin_member_active", { p_member: member }) === true
      ? member
      : null;
  },
  credentials: async (member, provider) => {
    const key = await rpc("hive_admin_member_key", {
      p_member: member,
      p_provider: provider,
    });
    if (typeof key !== "string" || !key) return null;
    const models = await rpc("hive_admin_member_models", { p_member: member });
    const model = models?.[provider] ||
      (provider === "anthropic"
        ? Deno.env.get("BOTS_ANTHROPIC_MODEL") ?? "claude-sonnet-4-5"
        : Deno.env.get("BOTS_NOUS_MODEL") ?? "anthropic/claude-sonnet-4.6");
    return { key, model };
  },
  fetch,
}));
