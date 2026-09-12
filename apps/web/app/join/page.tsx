"use client";

import { Suspense, useEffect, useState } from "react";
import { useSearchParams } from "next/navigation";
import type { Session } from "@supabase/supabase-js";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav } from "@/components/RequireMember";
import { TOS_VERSION } from "@/lib/tos";

function Join() {
  const params = useSearchParams();
  const [code, setCode] = useState(params.get("code") ?? "");
  const [session, setSession] = useState<Session | null | undefined>(undefined);
  const [state, setState] = useState<"idle" | "busy" | "joined" | "member" | "error">("idle");
  const [msg, setMsg] = useState<string>("");
  const [tosAccepted, setTosAccepted] = useState(false);

  useEffect(() => {
    const sb = supabaseBrowser();
    sb.auth.getSession().then(({ data }) => setSession(data.session));
    const { data: sub } = sb.auth.onAuthStateChange((_e, s) => setSession(s));
    return () => sub.subscription.unsubscribe();
  }, []);

  async function signIn(provider: "google" | "apple") {
    await supabaseBrowser().auth.signInWithOAuth({
      provider,
      options: { redirectTo: `${location.origin}/auth/callback?next=${encodeURIComponent(`/join?code=${code}`)}` },
    });
  }

  async function redeem() {
    setState("busy");
    const { data, error } = await supabaseBrowser().rpc("hive_invite_redeem", { p_code: code, p_tos_version: TOS_VERSION });
    if (error) { setState("error"); setMsg(error.message.replace(/_/g, " ")); return; }
    const d = data as { status: string; invited_by?: string };
    if (d.status === "already_member") setState("member");
    else { setState("joined"); setMsg(d.invited_by ?? ""); }
  }

  const wrap = { maxWidth: 560, margin: "48px auto", padding: "0 24px", lineHeight: 1.5 } as const;
  const btn = { padding: "8px 14px", cursor: "pointer" } as const;

  if (session === undefined) return <><Nav /><p style={wrap}>…</p></>;

  if (state === "joined" || state === "member") {
    return (
      <>
        <Nav />
        <main style={wrap}>
          <h1>Welcome to the Hive</h1>
          <p>{state === "joined" ? `You're in${msg ? `, invited by ${msg}` : ""}. You have a wallet and can see every project.` : "You're already a member."}</p>
          <p>Next: <a href="/pair">pair a machine</a> to start earning, or <a href="/new">start chatting</a>.</p>
        </main>
      </>
    );
  }

  return (
    <>
    <Nav />
    <main style={wrap}>
      <h1>Join the Hive</h1>
      <p>Hive is invite-only. Enter your invite code, then sign in with the account you'll use.</p>
      <label style={{ display: "block", fontSize: 13, color: "var(--muted-strong)", marginTop: 16 }}>Invite code</label>
      <input value={code} onChange={(e) => setCode(e.target.value.trim().toLowerCase())} placeholder="e.g. k7m2xq9r4t"
             style={{ display: "block", padding: 8, width: "100%", boxSizing: "border-box", fontFamily: "ui-monospace, monospace" }} />
      {!session ? (
        <div style={{ marginTop: 16 }}>
          <button onClick={() => signIn("google")} style={btn} disabled={code.length < 6}>Continue with Google</button>{" "}
          <button onClick={() => signIn("apple")} style={btn} disabled={code.length < 6}>Continue with Apple</button>
        </div>
      ) : (
        <div style={{ marginTop: 16 }}>
          <p style={{ fontSize: 13, color: "var(--muted-strong)" }}>Signed in as {session.user.email}.</p>
          <label style={{ display: "flex", alignItems: "flex-start", gap: 8, marginTop: 12, fontSize: 13, color: "var(--muted-strong)" }}>
            <input type="checkbox" checked={tosAccepted} onChange={(e) => setTosAccepted(e.target.checked)} style={{ marginTop: 2 }} />
            <span>I&apos;ve read and agree to the <a href="/terms" target="_blank" rel="noreferrer">Terms of Service</a>.</span>
          </label>
          <button onClick={redeem} style={{ ...btn, marginTop: 12 }} disabled={state === "busy" || code.length < 6 || !tosAccepted}>
            {state === "busy" ? "Joining…" : "Join with this code"}
          </button>
        </div>
      )}
      {state === "error" && <p style={{ color: "var(--danger)", marginTop: 12 }}>{msg}</p>}
    </main>
    </>
  );
}

export default function JoinPage() {
  return <Suspense fallback={<p style={{ padding: 24 }}>…</p>}><Join /></Suspense>;
}
