"use client";

import { useEffect, useState } from "react";
import type { Session } from "@supabase/supabase-js";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav } from "@/components/RequireMember";

type Status = {
  members: number; nodes_online: number; nodes_total: number; models_online: number; projects: number;
  cards: Record<string, number>; honey_paid_24h: number; tokens_24h: number;
  recent: { at: string; card: string; project: string; node: string | null; tokens: number; honey: number }[];
};

function Landing() {
  const signIn = (provider: "google" | "apple") =>
    supabaseBrowser().auth.signInWithOAuth({ provider, options: { redirectTo: `${location.origin}/auth/callback?next=/` } });
  return (
    <main style={{ maxWidth: 640, margin: "96px auto", padding: "0 24px", lineHeight: 1.55 }}>
      <h1 style={{ fontSize: 40, margin: "0 0 8px" }}>OH Hive</h1>
      <p style={{ fontSize: 18, color: "#444", marginTop: 0 }}>
        Office Hours Global and Loki&apos;s Lab, pooling the computers we already own into one machine that makes things.
      </p>
      <p style={{ color: "#666" }}>
        Share idle time, earn <strong>$honey</strong>. Spend it on projects — text, code, images, video, audio — worked by the whole Hive.
        Invite-only.
      </p>
      <div style={{ marginTop: 24 }}>
        <button onClick={() => signIn("google")} style={btn}>Sign in with Google</button>{" "}
        <button onClick={() => signIn("apple")} style={btn}>Sign in with Apple</button>
      </div>
      <p style={{ fontSize: 13, color: "#777", marginTop: 24 }}>Have an invite code? <a href="/join">Join here.</a></p>
    </main>
  );
}

function Pulse() {
  const [s, setS] = useState<Status | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    const load = () => supabaseBrowser().rpc("hive_status").then(({ data, error }) => { if (error) setErr(error.message); else setS(data as Status); });
    load(); const t = setInterval(load, 10000); return () => clearInterval(t);
  }, []);
  if (err) return <p style={{ padding: 24, color: "#b00020" }}>{err} — <a href="/join">not a member yet?</a></p>;
  if (!s) return <p style={{ padding: 24 }}>…</p>;
  const c = s.cards ?? {};
  return (
    <main style={{ maxWidth: 900, margin: "0 auto", padding: 24 }}>
      <h1 style={{ margin: "8px 0 16px" }}>The Hive, right now</h1>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(150px, 1fr))", gap: 12 }}>
        <Stat n={s.nodes_online} label={`nodes online of ${s.nodes_total}`} accent={s.nodes_online > 0} />
        <Stat n={s.models_online} label="models available" />
        <Stat n={s.members} label="members" />
        <Stat n={s.projects} label="projects" />
        <Stat n={(c.ready ?? 0) + (c.running ?? 0)} label={`cards queued · ${c.running ?? 0} running`} />
        <Stat n={c.review ?? 0} label="awaiting review" accent={(c.review ?? 0) > 0} />
        <Stat n={Number(s.tokens_24h).toLocaleString()} label="tokens generated, 24h" />
        <Stat n={Number(s.honey_paid_24h).toFixed(2)} label="$honey paid out, 24h" />
      </div>

      <div style={{ display: "flex", gap: 12, marginTop: 24, flexWrap: "wrap" }}>
        <a href="/new" style={cta}>Start a project</a>
        <a href="/projects" style={cta}>Browse the Hive</a>
        <a href="/pair" style={cta}>Pair a machine</a>
      </div>

      <h2 style={{ fontSize: 16, marginTop: 32 }}>Recent work</h2>
      {s.recent.length === 0 && <p style={{ color: "#666" }}>Nothing yet. Fund a project and the nodes will get to it.</p>}
      {s.recent.map((r, i) => (
        <div key={i} style={{ fontSize: 13, padding: "6px 0", borderBottom: "1px solid #eee", display: "flex", gap: 12 }}>
          <span style={{ color: "#777", whiteSpace: "nowrap" }}>{new Date(r.at).toLocaleTimeString()}</span>
          <span style={{ flex: 1 }}><strong>{r.node ?? "?"}</strong> finished “{r.card}” for {r.project}</span>
          <span style={{ color: "#2a7", whiteSpace: "nowrap" }}>+{Number(r.honey).toFixed(4)} · {r.tokens} tok</span>
        </div>
      ))}
    </main>
  );
}

function Stat({ n, label, accent }: { n: number | string; label: string; accent?: boolean }) {
  return (
    <div style={{ background: "#fff", border: "1px solid #e6e2d6", borderRadius: 8, padding: 14 }}>
      <div style={{ fontSize: 28, fontWeight: 600, color: accent ? "#c98a00" : "#1a1a1a" }}>{n}</div>
      <div style={{ fontSize: 12, color: "#777" }}>{label}</div>
    </div>
  );
}

const btn = { padding: "10px 16px", cursor: "pointer", fontSize: 15 } as const;
const cta = { padding: "10px 16px", background: "#f5c542", color: "#1a1a1a", borderRadius: 6, textDecoration: "none", fontWeight: 600 } as const;

export default function Home() {
  const [session, setSession] = useState<Session | null | undefined>(undefined);
  useEffect(() => {
    const sb = supabaseBrowser();
    sb.auth.getSession().then(({ data }) => setSession(data.session));
    const { data: sub } = sb.auth.onAuthStateChange((_e, s) => setSession(s));
    return () => sub.subscription.unsubscribe();
  }, []);
  if (session === undefined) return null;
  if (!session) return <Landing />;
  return (<><Nav /><Pulse /></>);
}

