"use client";

import { Suspense, useEffect } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { supabaseBrowser } from "@/lib/supabase";
import { safeAuthReturnPath } from "@/lib/auth-return-path";

/** OAuth landing: Supabase's PKCE flow returns ?code=…; exchange it, then bounce to `next`. */
function Callback() {
  const router = useRouter();
  const params = useSearchParams();
  useEffect(() => {
    const code = params.get("code");
    const next = safeAuthReturnPath(params.get("next"), location.origin);
    const sb = supabaseBrowser();
    (code ? sb.auth.exchangeCodeForSession(code) : Promise.resolve()).finally(() => router.replace(next));
  }, [params, router]);
  return <p style={{ padding: 24 }}>Signing you in…</p>;
}

export default function AuthCallback() {
  return (
    <Suspense fallback={<p style={{ padding: 24 }}>Signing you in…</p>}>
      <Callback />
    </Suspense>
  );
}
