"use client";

import { useEffect } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { supabaseBrowser } from "@/lib/supabase";

/** OAuth landing: Supabase's PKCE flow returns ?code=…; exchange it, then bounce to `next`. */
export default function AuthCallback() {
  const router = useRouter();
  const params = useSearchParams();
  useEffect(() => {
    const code = params.get("code");
    const next = params.get("next") ?? "/";
    const sb = supabaseBrowser();
    (code ? sb.auth.exchangeCodeForSession(code) : Promise.resolve()).finally(() => router.replace(next));
  }, [params, router]);
  return <p style={{ padding: 24 }}>Signing you in…</p>;
}
