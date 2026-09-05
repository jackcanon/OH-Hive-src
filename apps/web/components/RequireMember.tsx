"use client";

import { useEffect, useState, type ReactNode } from "react";
import type { Session } from "@supabase/supabase-js";
import { supabaseBrowser } from "@/lib/supabase";

/** Gate: signed in with Supabase, else show provider buttons. Membership is enforced by RLS/RPC. */
export function RequireMember({ children, next }: { children: (s: Session) => ReactNode; next: string }) {
  const [session, setSession] = useState<Session | null | undefined>(undefined);
  useEffect(() => {
    const sb = supabaseBrowser();
    sb.auth.getSession().then(({ data }) => setSession(data.session));
    const { data: sub } = sb.auth.onAuthStateChange((_e, s) => setSession(s));
    return () => sub.subscription.unsubscribe();
  }, []);
  if (session === undefined) return <p style={{ padding: 24 }}>…</p>;
  if (!session) {
    const signIn = (provider: "google" | "apple") =>
      supabaseBrowser().auth.signInWithOAuth({ provider, options: { redirectTo: `${location.origin}/auth/callback?next=${next}` } });
    return (
      <main style={{ maxWidth: 560, margin: "48px auto", padding: "0 24px" }}>
        <h1>Sign in</h1>
        <p>The Hive is invite-only. Sign in with the account you were invited with.</p>
        <button onClick={() => signIn("google")} style={{ padding: "8px 14px", cursor: "pointer" }}>Continue with Google</button>{" "}
        <button onClick={() => signIn("apple")} style={{ padding: "8px 14px", cursor: "pointer" }}>Continue with Apple</button>
      </main>
    );
  }
  return <>{children(session)}</>;
}

export function Nav() {
  return (
    <nav style={{ display: "flex", gap: 16, padding: "12px 24px", borderBottom: "1px solid #e6e2d6", fontSize: 14 }}>
      <a href="/" style={{ fontWeight: 600 }}>OH Hive</a>
      <a href="/projects">Projects</a>
      <a href="/new">Start a project</a>
      <a href="/wallet">Wallet</a>
      <a href="/pair">Pair a machine</a>
      <a href="/settings" style={{ marginLeft: "auto" }}>Settings</a>
    </nav>
  );
}

export const honey = (n: number | string | null | undefined) =>
  `${Number(n ?? 0).toLocaleString(undefined, { maximumFractionDigits: 4 })} $honey`;
