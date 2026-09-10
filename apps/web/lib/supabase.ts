import { createBrowserClient } from "@supabase/ssr";

/**
 * Browser client for the shared Cmd Work Supabase project (ADR-001 D9/D10).
 * Hive tables live in schema `hive` (D32). Until `hive` is added to the
 * project's Exposed Schemas, call the `public.hive_*` RPC wrappers instead
 * of `.schema("hive")`.
 */
export function supabaseBrowser() {
  const url = process.env.NEXT_PUBLIC_SUPABASE_URL;
  const key = process.env.NEXT_PUBLIC_SUPABASE_ANON_KEY;
  if (!url || !key) throw new Error("Supabase env not set — copy apps/web/.env.example to .env.local");
  return createBrowserClient(url, key);
}
