import { createBrowserClient } from "@supabase/ssr";

/**
 * Browser client for the shared Cmd Work Supabase project. All OH Hive tables
 * live in schema `hive` (ADR-001 D32) — always pass `.schema("hive")`.
 */
export function supabaseBrowser() {
  const url = process.env.NEXT_PUBLIC_SUPABASE_URL;
  const key = process.env.NEXT_PUBLIC_SUPABASE_ANON_KEY;
  if (!url || !key) throw new Error("Supabase env not set — copy apps/web/.env.example to .env.local");
  return createBrowserClient(url, key, { db: { schema: "hive" } });
}
