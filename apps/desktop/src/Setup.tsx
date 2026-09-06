import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { enable } from "@tauri-apps/plugin-autostart";
import type { Snapshot } from "./App";

export type Rung = { model: string; min_bytes: number; download_bytes: number; why: string };
export type Assessment = {
  hardware: { cpu_model: string; cpu_cores: number; ram_bytes: number; gpu_model: string | null; vram_bytes: number | null; disk_free_bytes: number };
  budget_bytes: number;
  ollama: { running: boolean; version: string | null; installed_app: string | null; url: string };
  recommended: Rung | null; fits: Rung[]; present: string[]; suggest_server: boolean; os: string; arch: string;
};
type Progress = { phase: string; text: string; completed: number; total: number; done: boolean; error: string | null };

const gb = (b: number | null | undefined) => `${(Number(b ?? 0) / 1073741824).toFixed(b && b > 10 * 1073741824 ? 0 : 1)} GB`;

/** Turn a vanilla machine into a Hive member: assess → Ollama → model → pair → go. */
export function Setup({ s, refresh, run }: { s: Snapshot; refresh: () => void; run: (c: string, a?: Record<string, unknown>) => Promise<void> }) {
  const [a, setA] = useState<Assessment | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [prog, setProg] = useState<Progress | null>(null);
  const [busy, setBusy] = useState(false);
  const [choice, setChoice] = useState<string | null>(null);

  const assess = () => invoke<Assessment>("assess").then((x) => { setA(x); if (!choice) setChoice(x.recommended?.model ?? null); }).catch((e) => setErr(String(e)));
  useEffect(() => {
    assess();
    const un = listen<Progress>("setup", (e) => { setProg(e.payload); if (e.payload.done) setTimeout(assess, 500); });
    return () => { un.then((f) => f()); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const step = async (cmd: string, args?: Record<string, unknown>) => {
    setBusy(true); setErr(null); setProg(null);
    try { await invoke(cmd, args); } catch (e) { setErr(String(e)); }
    setBusy(false); await assess(); refresh();
  };

  if (!a) return <div className="card"><p className="muted">Looking at this machine…</p>{err && <div className="banner">{err}</div>}</div>;

  const hasModel = choice ? a.present.some((m) => m === choice || m.startsWith(choice + ":")) : a.present.length > 0;
  const stage: 1 | 2 | 3 | 4 = !a.ollama.running ? 1 : !hasModel ? 2 : !s.paired ? 3 : 4;
  const pct = prog && prog.total > 0 ? Math.round((prog.completed / prog.total) * 100) : null;

  return (
    <>
      <div className="card">
        <h2>This machine</h2>
        <div className="grid">
          <div><div style={{ fontWeight: 600 }}>{a.hardware.cpu_model}</div><div className="muted" style={{ fontSize: 12 }}>{a.hardware.cpu_cores} cores · {gb(a.hardware.ram_bytes)} memory</div></div>
          <div><div style={{ fontWeight: 600 }}>{a.hardware.gpu_model ?? "no GPU"}</div><div className="muted" style={{ fontSize: 12 }}>{gb(a.budget_bytes)} usable for models · {gb(a.hardware.disk_free_bytes)} free disk</div></div>
        </div>
      </div>

      {err && <div className="banner">{err}</div>}

      <Stage n={1} title="Ollama" active={stage === 1} done={a.ollama.running}>
        {a.ollama.running
          ? <p className="muted" style={{ margin: 0 }}>Running, version {a.ollama.version}.</p>
          : <>
              <p className="muted" style={{ marginTop: 0 }}>{a.ollama.installed_app ? `Ollama is installed (${a.ollama.installed_app}) but not running.` : "Ollama runs the models. I can install it for you — nothing to type."}</p>
              <div className="row">
                <button className="primary" disabled={busy} onClick={() => step("ollama_install")}>{a.ollama.installed_app ? "Start Ollama" : "Install Ollama"}</button>
                <a href="#" className="muted" style={{ fontSize: 12 }} onClick={(e) => { e.preventDefault(); openUrl("https://ollama.com/download"); }}>or download it yourself</a>
              </div>
            </>}
      </Stage>

      <Stage n={2} title="Model" active={stage === 2} done={stage > 2}>
        {a.fits.length === 0
          ? <p className="muted" style={{ margin: 0 }}>Not enough memory for any of the Hive's models. This machine can still help as a regional server (disk, not compute).</p>
          : <>
              <p className="muted" style={{ marginTop: 0 }}>Most capable model that fits {gb(a.budget_bytes)}:</p>
              {a.fits.map((r) => (
                <label key={r.model} style={{ display: "flex", gap: 10, alignItems: "flex-start", padding: "6px 0", cursor: "pointer" }}>
                  <input type="radio" name="model" style={{ width: "auto", marginTop: 4 }} checked={choice === r.model} onChange={() => setChoice(r.model)} />
                  <span><strong>{r.model}</strong> {a.present.some((m) => m === r.model || m.startsWith(r.model + ":")) && <span className="ok" style={{ fontSize: 12 }}>· already here</span>}<br /><span className="muted" style={{ fontSize: 12 }}>{r.why} · {gb(r.download_bytes)} download</span></span>
                </label>
              ))}
              {stage === 2 && choice && <button className="primary" disabled={busy || !a.ollama.running} onClick={() => step("ollama_pull", { model: choice })} style={{ marginTop: 8 }}>Download {choice}</button>}
              {stage > 2 && choice && !a.present.some((m) => m === choice || m.startsWith(choice + ":")) && <button disabled={busy} onClick={() => step("ollama_pull", { model: choice })} style={{ marginTop: 8 }}>Switch to {choice}</button>}
            </>}
      </Stage>

      {prog && !prog.done && (
        <div className="card">
          <div className="row"><span style={{ fontSize: 13 }}>{prog.text}</span><span className="muted" style={{ fontSize: 12 }}>{pct !== null ? `${pct}%` : ""}</span></div>
          <div style={{ height: 6, background: "var(--surface-2)", borderRadius: 3, marginTop: 8 }}><div style={{ height: 6, width: `${pct ?? 5}%`, background: "var(--gold)", borderRadius: 3, transition: "width .3s" }} /></div>
          {prog.total > 0 && <div className="muted" style={{ fontSize: 11, marginTop: 4 }}>{gb(prog.completed)} of {gb(prog.total)}</div>}
        </div>
      )}

      <Stage n={3} title="Pair with your account" active={stage === 3} done={s.paired}>
        {s.paired
          ? <p className="muted" style={{ margin: 0 }}>Paired{s.summary?.node ? ` as “${s.summary.node.display_name}”` : ""}.</p>
          : <PairInline s={s} run={run} busy={busy} suggestServer={a.suggest_server} />}
      </Stage>

      <Stage n={4} title="Go" active={stage === 4} done={false}>
        <p className="muted" style={{ marginTop: 0 }}>Start working now, open at login so this machine stays in the Hive{a.suggest_server ? ", and consider the Server section — this machine has the disk for it" : ""}.</p>
        <div className="row">
          <button className="primary" disabled={stage !== 4 || busy} onClick={async () => {
            setBusy(true);
            try { await invoke("worker_start"); } catch (e) { setErr(String(e)); }
            try { await enable(); } catch { /* dev builds */ }
            try { await invoke("setup_finish"); } catch { /* ignore */ }
            setBusy(false); refresh();
          }}>Start working</button>
          <button disabled={busy} onClick={() => run("setup_finish")}>Skip for now</button>
        </div>
      </Stage>
    </>
  );
}

function Stage({ n, title, active, done, children }: { n: number; title: string; active: boolean; done: boolean; children: React.ReactNode }) {
  return (
    <div className="card" style={{ opacity: active || done ? 1 : 0.55, borderColor: active ? "var(--gold)" : "var(--border)" }}>
      <h2><span style={{ display: "inline-block", width: 18, height: 18, borderRadius: 9, textAlign: "center", lineHeight: "18px", fontSize: 11, marginRight: 8, background: done ? "var(--ok)" : active ? "var(--gold)" : "var(--surface-2)", color: done || active ? "var(--gold-fg)" : "var(--muted)" }}>{done ? "✓" : n}</span>{title}</h2>
      {children}
    </div>
  );
}

export function PairInline({ s, run, busy, suggestServer }: { s: Snapshot; run: (c: string, a?: Record<string, unknown>) => Promise<void>; busy: boolean; suggestServer?: boolean }) {
  const p = s.pairing;
  return !p ? (
    <>
      <p className="muted" style={{ marginTop: 0 }}>You'll get a short code to enter at ohghive.com — the key lands here automatically.{suggestServer ? " On the pairing page, pick “Compute and server” if you want this machine to hold artifacts too." : ""}</p>
      <button className="primary" disabled={busy} onClick={() => run("pair_begin")}>Get a pairing code</button>
    </>
  ) : (
    <>
      <p className="muted" style={{ margin: 0 }}>Enter this code at <a href="#" onClick={(e) => { e.preventDefault(); openUrl(p.url); }}>{p.url}</a></p>
      <div className="code">{p.code}</div>
      <p className="muted" style={{ fontSize: 12 }}>Waiting… good for {Math.round(p.expires_in_seconds / 60)} minutes.</p>
      <div className="row">
        <button className="primary" onClick={() => openUrl(p.url)}>Open ohghive.com/pair</button>
        <button onClick={() => run("pair_cancel")}>Cancel</button>
      </div>
    </>
  );
}
