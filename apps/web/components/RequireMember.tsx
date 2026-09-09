"use client";

import { useEffect, useState, type ReactNode } from "react";
import type { Session } from "@supabase/supabase-js";
import { supabaseBrowser } from "@/lib/supabase";
import { HoneyMark, Honey } from "@/components/Honey";
import { ThemeToggle } from "@/components/ThemeToggle";

/** Gate: signed in with Supabase, else show provider buttons. Membership is enforced by RLS/RPC. */
export function RequireMember({ children, next }: { children: (s: Session) => ReactNode; next: string }) {
  const [session, setSession] = useState<Session | null | undefined>(undefined);
  useEffect(() => {
    const sb = supabaseBrowser();
    sb.auth.getSession().then(({ data }) => setSession(data.session));
    const { data: sub } = sb.auth.onAuthStateChange((_e, s) => setSession(s));
    return () => sub.subscription.unsubscribe();
  }, []);
  if (session === undefined) return <p style={{ padding: 24 }}>Checking your session…</p>;
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
  const [open, setOpen] = useState(false);
  return (
    <nav className="hive">
      <a href="/" className="brand"><HoneyMark height={20} title="OH Hive" /> OH Hive</a>
      <button
        type="button"
        className="hive-toggle"
        aria-label="Toggle navigation menu"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <svg viewBox="0 0 18 14" width="18" height="14" fill="none" xmlns="http://www.w3.org/2000/svg">
          <path d="M0 1h18M0 7h18M0 13h18" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
        </svg>
      </button>
      <div className={open ? "hive-links open" : "hive-links"}>
        <a href="/projects">Projects</a>
        <a href="/new">Start a project</a>
        <a href="/wallet">Wallet</a>
        <a href="/pair">Pair a machine</a>
        <a href="/help">Help</a>
        <a href="/settings" style={{ marginLeft: "auto" }}>Settings</a>
      </div>
      <ThemeToggle />
    </nav>
  );
}

/** An amount of Honey with the gold symbol. Kept under the old name so every page's `honey(x)` keeps working. */
export const honey = (n: number | string | null | undefined) => <Honey n={n} />;
