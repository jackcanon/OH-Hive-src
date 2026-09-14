import { createClient } from "npm:@supabase/supabase-js@2";
import { createHandler } from "./handler.ts";

const admin = createClient(Deno.env.get("SUPABASE_URL")!, Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!, { auth: { persistSession: false, autoRefreshToken: false } });
Deno.serve(createHandler((name, args) => admin.rpc(name, args), fetch, {
  anthropic: Deno.env.get("CODE_BRAIN_ANTHROPIC_MODEL") ?? Deno.env.get("INTERVIEW_MODEL") ?? "claude-sonnet-4-5",
  nous: Deno.env.get("CODE_BRAIN_NOUS_MODEL") ?? Deno.env.get("INTERVIEW_NOUS_MODEL") ?? "anthropic/claude-sonnet-4.6",
  openai: Deno.env.get("CODE_BRAIN_OPENAI_MODEL") ?? Deno.env.get("INTERVIEW_OPENAI_MODEL") ?? "gpt-5",
}));
