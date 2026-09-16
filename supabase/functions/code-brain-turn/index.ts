import { createClient } from "npm:@supabase/supabase-js@2";
import { createHandler } from "./handler.ts";

const admin = createClient(Deno.env.get("SUPABASE_URL")!, Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!, { auth: { persistSession: false, autoRefreshToken: false } });

// USD per million tokens, as `CODE_BRAIN_PRICE_<PROVIDER>="<in>,<out>"` (e.g. "3,15"). A provider
// with no valid pair stays unpriced rather than guessed: its turns are still recorded with real
// token counts, `usage_priced: false`, and a 0 dollar estimate, which keeps it out of the monthly
// ceiling. Anthropic falls back to the figures the interviewer already uses.
const price = (provider: string, fallback?: { in: number; out: number }) => {
  const [i, o] = (Deno.env.get(`CODE_BRAIN_PRICE_${provider.toUpperCase()}`) ?? "").split(",").map(Number);
  return Number.isFinite(i) && Number.isFinite(o) && i >= 0 && o >= 0 ? { in: i, out: o } : fallback;
};
Deno.serve(createHandler((name, args) => admin.rpc(name, args), fetch, {
  anthropic: Deno.env.get("CODE_BRAIN_ANTHROPIC_MODEL") ?? Deno.env.get("INTERVIEW_MODEL") ?? "claude-sonnet-4-5",
  nous: Deno.env.get("CODE_BRAIN_NOUS_MODEL") ?? Deno.env.get("INTERVIEW_NOUS_MODEL") ?? "anthropic/claude-sonnet-4.6",
  openai: Deno.env.get("CODE_BRAIN_OPENAI_MODEL") ?? Deno.env.get("INTERVIEW_OPENAI_MODEL") ?? "gpt-5",
}, {
  ...(price("anthropic", { in: 3.0, out: 15.0 }) ? { anthropic: price("anthropic", { in: 3.0, out: 15.0 })! } : {}),
  ...(price("openai") ? { openai: price("openai")! } : {}),
  ...(price("nous") ? { nous: price("nous")! } : {}),
}));
