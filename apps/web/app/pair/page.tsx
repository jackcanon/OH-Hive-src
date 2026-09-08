"use client";

import { useEffect, useState } from "react";
import type { Session } from "@supabase/supabase-js";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav } from "@/components/RequireMember";

const TOS_VERSION = "v1";

type Peek = { code: string; hint: Record<string, unknown>; expires_at: string } | null;

export default function PairPage() {
  const [session, setSession] = useState<Session | null>(null);
  const [code, setCode] = useState("");
  const [peek, setPeek] = useState<Peek>(null);
  const [name, setName] = useState("");
  const [role, setRole] = useState<"compute" | "regional_server" | "compute_and_server">("compute");
  const [allowInternet, setAllowInternet] = useState(false);
  const [tools, setTools] = useState<"inference_only" | "sandboxed_tools">("sandboxed_tools");
  const [tos, setTos] = useState(false);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [done, setDone] = useState<{ node_id: string; display_name: string } | null>(null);

  useEffect(() => {
    const sb = supabaseBrowser();
    sb.auth.getSession().then(({ data }) => setSession(data.session));
    const { data: sub } = sb.auth.onAuthStateChange((_e, s) => setSession(s));
    return () => sub.subscription.unsubscribe();
  }, []);

  async function signIn(provider: "google" | "apple") {
    const sb = supabaseBrowser();
    await sb.auth.signInWithOAuth({
      provider,
      options: { redirectTo: `${location.origin}/auth/callback?next=/pair` },
    });
  }

  async function lookup() {
    setMsg(null);
    setPeek(null);
    const sb = supabaseBrowser();
    const { data, error } = await sb.rpc("hive_pair_peek", { p_code: code });
    if (error) return setMsg(error.message);
    if (!data) return setMsg("That code isn't waiting to be claimed. Check it, or run `hive pair` again.");
    setPeek(data as Peek);
    const h = (data as { hint?: { hostname?: string } }).hint;
    if (h?.hostname && !name) setName(String(h.hostname));
  }

  async function claim() {
    setBusy(true);
    setMsg(null);
    const sb = supabaseBrowser();
    const { data, error } = await sb.rpc("hive_pair_claim", {
      p_code: code,
      p_display_name: name.trim(),
      p_role: role,
      p_allow_internet: allowInternet,
      p_tools_level: tools,
      p_tos_version: TOS_VERSION,
    });
    setBusy(false);
    if (error) return setMsg(error.message.replace(/_/g, " "));
    setDone(data as { node_id: string; display_name: string });
  }

  const wrap = { maxWidth: 560, margin: "48px auto", padding: "0 24px", lineHeight: 1.5 } as const;

  if (!session) {
    return (
      <>
        <Nav />
        <main style={wrap}>
          <h1>Pair a machine</h1>
          <p>Sign in with the account you use for the Hive, then enter the code your machine is showing.</p>
          <button onClick={() => signIn("google")} style={btn}>Continue with Google</button>{" "}
          <button onClick={() => signIn("apple")} style={btn}>Continue with Apple</button>
        </main>
      </>
    );
  }

  if (done) {
    return (
      <>
        <Nav />
        <main style={wrap}>
          <h1>Paired</h1>
          <p>
            <strong>{done.display_name}</strong> is now your node. The machine will pick up its key within a few
            seconds and can check in with <code>hive check-in --stay</code>.
          </p>
          <p><a href="/wallet">Manage your nodes</a></p>
        </main>
      </>
    );
  }

  return (
    <>
    <Nav />
    <main style={wrap}>
      <h1>Pair a machine</h1>
      <p>On the machine, run <code>hive pair</code> and enter the code it shows.</p>

      <label style={lbl}>Pairing code</label>
      <div style={{ display: "flex", gap: 8 }}>
        <input
          value={code}
          onChange={(e) => setCode(e.target.value.toUpperCase())}
          placeholder="HK7-3PQ"
          maxLength={7}
          style={{ ...inp, fontFamily: "ui-monospace, monospace", fontSize: 22, letterSpacing: 2, width: 160 }}
        />
        <button onClick={lookup} style={btn} disabled={code.length < 6}>Look up</button>
      </div>

      {peek && (
        <>
          <p style={{ marginTop: 16, color: "var(--muted-strong)" }}>
            Machine reports: <code>{JSON.stringify(peek.hint)}</code>
          </p>

          <label style={lbl}>Name this node</label>
          <input value={name} onChange={(e) => setName(e.target.value)} style={inp} placeholder="Heimdall" />

          <label style={lbl}>Role</label>
          <select value={role} onChange={(e) => setRole(e.target.value as typeof role)} style={inp}>
            <option value="compute">Compute node — runs models</option>
            <option value="regional_server">Regional server — relay, storage, model cache</option>
            <option value="compute_and_server">Both</option>
          </select>

          <fieldset style={{ border: "1px solid var(--border)", padding: 12, marginTop: 16 }}>
            <legend style={{ padding: "0 6px" }}>Trust</legend>
            <label style={{ display: "block" }}>
              <input type="checkbox" checked={allowInternet} onChange={(e) => setAllowInternet(e.target.checked)} />{" "}
              Allow projects to reach the internet from this machine <span style={{ color: "var(--muted)" }}>(off by default)</span>
            </label>
            <label style={{ display: "block", marginTop: 8 }}>
              <input type="radio" checked={tools === "sandboxed_tools"} onChange={() => setTools("sandboxed_tools")} />{" "}
              Sandboxed tools (recommended) — agents get a scratch folder and sandboxed code execution
            </label>
            <label style={{ display: "block" }}>
              <input type="radio" checked={tools === "inference_only"} onChange={() => setTools("inference_only")} />{" "}
              Inference only — model in, tokens out, nothing else
            </label>
          </fieldset>

          <label style={{ display: "block", marginTop: 16 }}>
            <input type="checkbox" checked={tos} onChange={(e) => setTos(e.target.checked)} />{" "}
            I provide compute, I earn Honey, I claim no rights in project outputs, and I won&apos;t redistribute
            owner-only material I can see. <span style={{ color: "var(--muted)" }}>(Contributor terms {TOS_VERSION})</span>
          </label>

          <button onClick={claim} disabled={busy || !tos || !name.trim()} style={{ ...btn, marginTop: 16 }}>
            {busy ? "Pairing…" : "Pair this machine"}
          </button>
        </>
      )}

      {msg && <p style={{ color: "var(--danger)", marginTop: 12 }}>{msg}</p>}
    </main>
    </>
  );
}

const btn = { padding: "8px 14px", cursor: "pointer" } as const;
const inp = { display: "block", padding: 8, width: "100%", boxSizing: "border-box" } as const;
const lbl = { display: "block", marginTop: 16, fontSize: 13, color: "var(--muted-strong)" } as const;
