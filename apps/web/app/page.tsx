"use client";

import { useEffect, useState } from "react";
import type { Session } from "@supabase/supabase-js";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav } from "@/components/RequireMember";
import { ThemeToggle } from "@/components/ThemeToggle";
import { HoneyMark } from "@/components/Honey";

type Status = {
  members: number; nodes_online: number; nodes_total: number; models_online: number; projects: number;
  cards: Record<string, number>; honey_paid_24h: number; tokens_24h: number;
  recent: { at: string; card: string; project: string; node: string | null; tokens: number; honey: number }[];
  servers_online?: number;
  coordinator?: { name: string | null; generation: number } | null;
  backup?: { hash: string; created_at: string; age_hours: number; replicas: number; replication: number } | null;
};

type PresenceEvent = {
  id: number; kind: "node" | "regional_server"; name: string; region: string | null;
  from_status: string | null; to_status: string; at: string;
};

// Jack, 2026-09-12: "I feel like we should show our metrics, ms between whichever server they are
// connected to, all of that kind of stuff." The control plane is fully hub-centric (every node and
// regional server talks straight to Supabase for everything), so "ms to whichever server" is each
// participant's own round-trip time to this hub, measured on its heartbeat calls
// (crates/ohhive-core/src/hub.rs) and surfaced here via hive.connectivity_summary().
type RegionRtt = { region: string; count: number; avg_rtt_ms: number | null; min_rtt_ms: number | null; max_rtt_ms: number | null };
type RttNode = { node_id: string; name: string; region: string | null; role: string; presence: string; rtt_ms: number | null; last_heartbeat: string | null };
type ConnectivitySummary = { by_region: RegionRtt[]; nodes: RttNode[] };

function rttColor(ms: number | null): string {
  if (ms == null) return "var(--muted)";
  if (ms < 150) return "var(--ok)";
  if (ms < 400) return "var(--gold)";
  return "var(--danger)";
}

function statusWord(s: string | null) {
  if (!s) return "new";
  return s.replace(/_/g, " ");
}

function isUpTransition(s: string) {
  return s === "checked_in" || s === "online";
}

function Connectivity() {
  const [events, setEvents] = useState<PresenceEvent[] | null>(null);
  const [summary, setSummary] = useState<ConnectivitySummary | null>(null);
  useEffect(() => {
    const load = () => {
      supabaseBrowser().rpc("hive_presence_recent", { p_limit: 30 }).then(({ data }) => {
        if (Array.isArray(data)) setEvents(data as PresenceEvent[]);
      });
      supabaseBrowser().rpc("hive_connectivity_summary").then(({ data }) => {
        if (data) setSummary(data as ConnectivitySummary);
      });
    };
    load(); const t = setInterval(load, 15000); return () => clearInterval(t);
  }, []);
  const nodesWithRtt = [...(summary?.nodes ?? [])].filter((n) => n.rtt_ms != null).sort((a, b) => (b.rtt_ms ?? 0) - (a.rtt_ms ?? 0));
  if ((!events || events.length === 0) && nodesWithRtt.length === 0) return null;
  return (
    <>
      <h2 style={{ fontSize: 16, marginTop: 32 }}>Connectivity</h2>
      <p style={{ fontSize: 12, color: "var(--muted)", marginTop: -8, marginBottom: 8 }}>
        Round-trip time from each node/server to the hub, plus who's coming online or dropping off.
      </p>

      {summary && summary.by_region.length > 0 && (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(140px, 1fr))", gap: 10, marginBottom: 16 }}>
          {summary.by_region.map((r) => (
            <div key={r.region} style={{ background: "var(--surface)", border: "1px solid var(--border)", borderRadius: 8, padding: 10 }}>
              <div style={{ fontSize: 20, fontWeight: 600, color: rttColor(r.avg_rtt_ms) }}>{r.avg_rtt_ms != null ? `${r.avg_rtt_ms}ms` : "—"}</div>
              <div style={{ fontSize: 11, color: "var(--muted)" }}>{r.region} · {r.count} node{r.count === 1 ? "" : "s"} · {r.min_rtt_ms}–{r.max_rtt_ms}ms range</div>
            </div>
          ))}
        </div>
      )}

      {nodesWithRtt.length > 0 && (
        <details style={{ marginBottom: 16 }}>
          <summary style={{ cursor: "pointer", fontSize: 13, color: "var(--muted-strong)" }}>Per-node latency ({nodesWithRtt.length})</summary>
          {nodesWithRtt.map((n) => (
            <div key={n.node_id} style={{ fontSize: 13, padding: "5px 0", borderBottom: "1px solid var(--border)", display: "flex", gap: 12, alignItems: "center" }}>
              <span style={{ flex: 1 }}>{n.name}{n.region ? <span style={{ color: "var(--muted)" }}> ({n.region})</span> : null}</span>
              <span style={{ color: rttColor(n.rtt_ms), fontWeight: 600, fontVariantNumeric: "tabular-nums" }}>{n.rtt_ms}ms</span>
            </div>
          ))}
        </details>
      )}

      {(events ?? []).map((e) => {
        const up = isUpTransition(e.to_status);
        const down = e.to_status === "checked_out" || e.to_status === "draining" || e.to_status === "offline";
        return (
          <div key={e.id} style={{ fontSize: 13, padding: "6px 0", borderBottom: "1px solid var(--border)", display: "flex", gap: 12 }}>
            <span style={{ color: "var(--muted)", whiteSpace: "nowrap" }}>{new Date(e.at).toLocaleTimeString()}</span>
            <span style={{ flex: 1 }}>
              <strong>{e.name}</strong>
              {e.region ? <span style={{ color: "var(--muted)" }}> ({e.region})</span> : null}
              {" "}{e.kind === "regional_server" ? "regional server" : "node"} went from{" "}
              <span style={{ color: "var(--muted-strong)" }}>{statusWord(e.from_status)}</span> to{" "}
              <span style={{ color: up ? "var(--ok)" : down ? "var(--danger)" : "var(--muted-strong)", fontWeight: 600 }}>{statusWord(e.to_status)}</span>
            </span>
          </div>
        );
      })}
    </>
  );
}

function Landing() {
  const signIn = (provider: "google" | "apple") =>
    supabaseBrowser().auth.signInWithOAuth({ provider, options: { redirectTo: `${location.origin}/auth/callback?next=/` } });
  return (
    <main style={{ maxWidth: 640, margin: "96px auto", padding: "0 24px", lineHeight: 1.55 }}>
      <div style={{ position: "fixed", top: 12, right: 16 }}><ThemeToggle /></div>
      <div style={{ display: "flex", alignItems: "center", gap: 14, marginBottom: 8 }}>
        <HoneyMark height={44} title="Hive" />
        <h1 style={{ fontSize: 40, margin: 0 }}>Hive</h1>
      </div>
      <p style={{ fontSize: 18, color: "var(--muted-strong)", marginTop: 0 }}>
        Our community, pooling the computers we already own into one machine that makes things.
      </p>
      <p style={{ color: "var(--muted-strong)" }}>
        Share idle time, earn <strong>Honey</strong>. Spend it on projects — text, code, images, video, audio — worked by the whole Hive.
        Invite-only.
      </p>
      <div style={{ marginTop: 24 }}>
        <button onClick={() => signIn("google")} style={btn}>Sign in with Google</button>{" "}
        <button onClick={() => signIn("apple")} style={btn}>Sign in with Apple</button>
      </div>
      <p style={{ fontSize: 13, color: "var(--muted)", marginTop: 24 }}>Have an invite code? <a href="/join">Join here.</a></p>
    </main>
  );
}

function Pulse() {
  const [s, setS] = useState<Status | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [notMember, setNotMember] = useState(false);
  useEffect(() => {
    const load = () => supabaseBrowser().rpc("hive_status").then(({ data, error }) => {
      if (error) setErr(error.message);
      else if (data == null) setNotMember(true);
      else setS(data as Status);
    });
    load(); const t = setInterval(load, 15000); return () => clearInterval(t);
  }, []);
  if (notMember) {
    return (
      <main style={{ maxWidth: 560, margin: "96px auto", padding: "0 24px", lineHeight: 1.55 }}>
        <h1 style={{ fontSize: 26 }}>You&apos;re signed in, but not a member yet</h1>
        <p style={{ color: "var(--muted-strong)" }}>Hive is invite-only. Enter the invite code you were sent to join the Hive.</p>
        <p><a href="/join" style={{ fontWeight: 600 }}>Join with your invite code →</a></p>
      </main>
    );
  }
  if (err) return <p style={{ padding: 24, color: "var(--danger)" }}>{err} — <a href="/join">not a member yet?</a></p>;
  if (!s) return <p style={{ padding: 24 }}>Loading the Hive…</p>;
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
        <Stat n={Number(s.honey_paid_24h).toFixed(2)} label="Honey paid out, 24h" />
      </div>

      {s.nodes_total >= 100 && (
        <p style={{ fontSize: 13, padding: "8px 12px", marginTop: 12, borderRadius: 6, background: "var(--warn-bg)", border: "1px solid var(--warn-border)", color: "var(--warn-fg)" }}>
          {s.nodes_total >= 150
            ? <>⚠ {s.nodes_total} nodes — past the ADR-013 (D72) threshold. Time to move the control plane off direct RPC onto the coordinator before it becomes an outage.</>
            : <>{s.nodes_total} nodes and climbing — ADR-013 (D72) calls for moving the control plane off direct RPC at 150 nodes. Worth scheduling before it's discovered the hard way.</>}
        </p>
      )}

      <p style={{ fontSize: 13, color: "var(--muted)", marginTop: 12 }}>
        {s.servers_online ?? 0} regional server{(s.servers_online ?? 0) === 1 ? "" : "s"} online
        {s.coordinator?.name ? <> · coordinator <strong style={{ color: "var(--muted-strong)" }}>{s.coordinator.name}</strong></> : " · no coordinator"}
        {" · "}
        {s.backup
          ? <>last backup <strong style={{ color: s.backup.age_hours > 30 ? "var(--danger)" : "var(--muted-strong)" }}>{s.backup.age_hours < 1 ? "under an hour" : `${Math.round(s.backup.age_hours)} h`} ago</strong>, {s.backup.replicas} of {s.backup.replication} copies</>
          : <span style={{ color: "var(--danger)" }}>no backup yet</span>}
      </p>

      <div style={{ display: "flex", gap: 12, marginTop: 24, flexWrap: "wrap" }}>
        <a href="/new" style={cta}>Chat</a>
        <a href="/projects" style={cta}>Browse the Hive</a>
        <a href="/pair" style={cta}>Pair a machine</a>
      </div>

      <h2 style={{ fontSize: 16, marginTop: 32 }}>Recent work</h2>
      {s.recent.length === 0 && <p style={{ color: "var(--muted-strong)" }}>Nothing yet. Fund a project and the nodes will get to it.</p>}
      {s.recent.map((r, i) => (
        <div key={i} style={{ fontSize: 13, padding: "6px 0", borderBottom: "1px solid var(--border)", display: "flex", gap: 12 }}>
          <span style={{ color: "var(--muted)", whiteSpace: "nowrap" }}>{new Date(r.at).toLocaleTimeString()}</span>
          <span style={{ flex: 1 }}><strong>{r.node ?? "?"}</strong> finished “{r.card}” for {r.project}</span>
          <span style={{ color: "var(--ok)", whiteSpace: "nowrap" }}>+{Number(r.honey).toFixed(4)} · {r.tokens} tok</span>
        </div>
      ))}

      <Connectivity />
    </main>
  );
}

function Stat({ n, label, accent }: { n: number | string; label: string; accent?: boolean }) {
  return (
    <div style={{ background: "var(--surface)", border: "1px solid var(--border)", borderRadius: 8, padding: 14 }}>
      <div style={{ fontSize: 28, fontWeight: 600, color: accent ? "var(--gold)" : "var(--fg)" }}>{n}</div>
      <div style={{ fontSize: 12, color: "var(--muted)" }}>{label}</div>
    </div>
  );
}

const btn = { padding: "10px 16px", cursor: "pointer", fontSize: 15 } as const;
const cta = { padding: "10px 16px", background: "var(--gold)", color: "var(--gold-fg)", borderRadius: 6, textDecoration: "none", fontWeight: 600 } as const;

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

