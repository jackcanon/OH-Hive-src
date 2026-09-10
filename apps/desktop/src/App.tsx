import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import { AboutSection, type AboutInfo } from "@hive/ui";
import { HoneyMark } from "./HoneyMark";
import { Setup, PairInline } from "./Setup";
import { Server } from "./Server";

export type Activity = { at: string; text: string; kind: string };
export type Snapshot = {
  version: string; paired: boolean; config_path: string; hub_url: string; llama_url: string; region: string | null;
  model: string | null; models: string[]; backend_ok: boolean; running: boolean; busy: boolean;
  pairing: { code: string; url: string; expires_in_seconds: number } | null;
  summary: {
    node: { display_name: string; region: string | null; presence: string; role: string } | null;
    earned: { total: number; last_24h: number; cards: number; tokens_out: number };
    wallet: number | null; queue: number; rate: number | null;
    recent: { at: string; card: string; project: string; honey: number; tokens_out: number }[];
  } | null;
  activity: Activity[]; error: string | null;
  server: { running: boolean; registered: boolean; coordinator: boolean; coordinator_name: string | null; blobs: number; used_bytes: number; last_backup: string | null; public_url: string; storage_gb: number; tier: string; operator: string; listen: string; data_dir: string };
  worker_enabled: boolean; server_enabled: boolean; setup_done: boolean;
  allow_internet: boolean; tools_level: "inference_only" | "sandboxed_tools";
  tunnel: { available: boolean; logged_in: boolean; hostname: string | null; running: boolean };
};

const TABS = ["Setup", "Node", "Server", "Earnings", "Settings", "About"] as const;
type Tab = (typeof TABS)[number];

function honey(n: number | null | undefined, d = 2) {
  return <span style={{ whiteSpace: "nowrap" }}><HoneyMark height={12} /> {Number(n ?? 0).toFixed(d)}</span>;
}

export function App() {
  const [s, setS] = useState<Snapshot | null>(null);
  const [tab, setTab] = useState<Tab | null>(null);
  const [about, setAbout] = useState<AboutInfo | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [busyBtn, setBusyBtn] = useState(false);

  const refresh = useCallback(() => {
    invoke<Snapshot>("snapshot").then((x) => { setS(x); if (x.error) setErr(x.error); }).catch((e) => setErr(String(e)));
  }, []);

  useEffect(() => {
    refresh();
    invoke<AboutInfo>("about").then(setAbout).catch(() => {});
    isEnabled().then(setAutostart).catch(() => setAutostart(null));
    const t = setInterval(refresh, 15000);
    const un1 = listen("changed", refresh);
    const un2 = listen<Activity>("activity", (e) => setS((p) => p ? { ...p, activity: [e.payload, ...p.activity].slice(0, 60) } : p));
    const un3 = listen("worker", () => setTimeout(refresh, 300));
    return () => { clearInterval(t); un1.then((f) => f()); un2.then((f) => f()); un3.then((f) => f()); };
  }, [refresh]);

  const run = async (cmd: string, args?: Record<string, unknown>) => {
    setBusyBtn(true); setErr(null);
    try { await invoke(cmd, args); } catch (e) { setErr(String(e)); }
    setBusyBtn(false); refresh();
  };

  if (!s) return <div className="wrap"><div className="drag" /><p className="muted">…</p></div>;
  const sum = s.summary;
  const cur: Tab = tab ?? (s.setup_done ? "Node" : "Setup");
  const visibleTabs = TABS.filter((t) => t !== "Setup" || !s.setup_done);

  return (
    <div>
      <div className="drag" />
      <div className="wrap">
        <div className="hero">
          <HoneyMark height={34} />
          <div>
            <h1>Hive</h1>
            <div className="muted" style={{ fontSize: 12 }}>
              <span className={"dot" + (s.running ? (s.busy ? " busy" : " on") : "")} />
              {!s.paired ? "not paired" : s.running ? (s.busy ? "working on a card" : "working — waiting for cards") : "not working"}
              {s.server.running ? (s.server.coordinator ? " · serving (coordinator)" : " · serving") : ""}
              {sum?.node ? ` · ${sum.node.display_name}` : ""}
            </div>
          </div>
        </div>

        {err && <div className="banner">{err}</div>}
        {s.paired && !s.backend_ok && <div className="banner">Ollama isn’t answering at {s.llama_url}. Start Ollama (or set the URL in Settings) to work.</div>}

        <div className="tabs">
          {visibleTabs.map((t) => <button key={t} className={t === cur ? "active" : ""} onClick={() => setTab(t)}>{t}</button>)}
        </div>

        {cur === "Setup" && <Setup s={s} refresh={refresh} run={run} />}

        {cur === "Server" && <Server s={s} run={run} busy={busyBtn} />}

        {cur === "Node" && (!s.paired ? <div className="card"><h2>Pair this Mac</h2><PairInline s={s} run={run} busy={busyBtn} /></div> : (
          <>
            <div className="card">
              <div className="row">
                <div>
                  <div style={{ fontWeight: 600 }}>{s.running ? "This Mac is in the Hive" : "This Mac is paired"}</div>
                  <div className="muted" style={{ fontSize: 12 }}>
                    {s.models.length} model{s.models.length === 1 ? "" : "s"} · using {s.model ?? (s.models[0] ?? "—")}{s.region ? ` · ${s.region}` : ""}
                  </div>
                </div>
                {s.running
                  ? <button className="danger" disabled={busyBtn} onClick={() => run("worker_stop")}>Stop</button>
                  : <button className="primary" disabled={busyBtn || !s.backend_ok} onClick={() => run("worker_start")}>Start working</button>}
              </div>
            </div>
            {sum && (
              <div className="grid">
                <div className="card"><div className="stat">{honey(sum.earned.last_24h, 3)}<small>earned, last 24 h</small></div></div>
                <div className="card"><div className="stat">{sum.queue}<small>cards waiting in the Hive</small></div></div>
              </div>
            )}
            <div className="card">
              <h2>Activity</h2>
              <div className="log">
                {s.activity.length === 0 && <div className="muted">Nothing yet.</div>}
                {s.activity.map((a, i) => (
                  <div key={i}><span className="at">{new Date(a.at).toLocaleTimeString()}</span><span className={a.kind}>{a.text}</span></div>
                ))}
              </div>
            </div>
          </>
        ))}

        {cur === "Earnings" && (
          !sum ? <p className="muted">Pair first.</p> : (
            <>
              <div className="grid">
                <div className="card"><div className="stat">{honey(sum.earned.total, 3)}<small>earned by this Mac, all time</small></div></div>
                <div className="card"><div className="stat">{honey(sum.wallet, 2)}<small>your wallet</small></div></div>
                <div className="card"><div className="stat">{sum.earned.cards}<small>cards finished</small></div></div>
                <div className="card"><div className="stat">{Number(sum.earned.tokens_out).toLocaleString()}<small>tokens generated</small></div></div>
              </div>
              <div className="card">
                <h2>Recent cards</h2>
                <div className="log">
                  {sum.recent.length === 0 && <div className="muted">No cards finished yet.</div>}
                  {sum.recent.map((r, i) => (
                    <div key={i}><span className="at">{new Date(r.at).toLocaleString()}</span><span style={{ flex: 1 }}>{r.card} <span className="muted">· {r.project}</span></span><span className="ok">+{Number(r.honey).toFixed(4)}</span></div>
                  ))}
                </div>
              </div>
              <p className="muted" style={{ fontSize: 12 }}>
                Rate: {sum.rate ?? "—"} Honey per output token. Full wallet at <a href="#" onClick={(e) => { e.preventDefault(); openUrl("https://ohghive.com/wallet"); }}>ohghive.com/wallet</a>.
              </p>
            </>
          )
        )}

        {cur === "Settings" && <Settings s={s} run={run} autostart={autostart} setAutostart={setAutostart} refresh={refresh} />}

        {cur === "About" && (
          <div className="card about">
            {about ? <AboutSection info={about} /> : <p>…</p>}
            <p style={{ marginTop: 12 }}>Config: <code style={{ fontSize: 11 }}>{s.config_path}</code></p>
          </div>
        )}
      </div>
    </div>
  );
}

function Settings({ s, run, autostart, setAutostart, refresh }: {
  s: Snapshot; run: (c: string, a?: Record<string, unknown>) => Promise<void>;
  autostart: boolean | null; setAutostart: (b: boolean) => void; refresh: () => void;
}) {
  const [llama, setLlama] = useState(s.llama_url);
  const [region, setRegion] = useState(s.region ?? "");
  return (
    <>
      <div className="card">
        <h2>Model</h2>
        <p className="muted" style={{ fontSize: 12, marginTop: 0 }}>Which local model takes cards. Cards that name a model override this.</p>
        <select value={s.model ?? ""} onChange={(e) => run("set_config", { key: "HIVE_MODEL", value: e.target.value })}>
          <option value="">Automatic (first available)</option>
          {s.models.map((m) => <option key={m} value={m}>{m}</option>)}
        </select>
        {s.running && <p className="muted" style={{ fontSize: 12 }}>Takes effect on the next card.</p>}
      </div>
      <div className="card">
        <h2>Launch at login</h2>
        <div className="row">
          <span className="muted" style={{ fontSize: 13 }}>Open Hive when you sign in, so the node is ready without a click.</span>
          <button disabled={autostart === null} onClick={async () => { try { autostart ? await disable() : await enable(); setAutostart(!autostart); } catch { /* plugin unavailable in dev */ } }}>
            {autostart ? "On" : "Off"}
          </button>
        </div>
      </div>
      <div className="card">
        <h2>Backend</h2>
        <label className="field">Ollama / llama-server URL</label>
        <input value={llama} onChange={(e) => setLlama(e.target.value)} onBlur={() => llama !== s.llama_url && run("set_config", { key: "HIVE_LLAMA_URL", value: llama })} />
        <label className="field">Region (optional, e.g. us-west)</label>
        <input value={region} onChange={(e) => setRegion(e.target.value)} onBlur={() => region !== (s.region ?? "") && run("set_config", { key: "HIVE_REGION", value: region })} />
        <p className="muted" style={{ fontSize: 12 }}>Hub: {s.hub_url}. <a href="#" onClick={(e) => { e.preventDefault(); refresh(); }}>Refresh</a></p>
      </div>
      <div className="card">
        <h2>Trust</h2>
        <p className="muted" style={{ fontSize: 12, marginTop: 0 }}>
          Whole-node choices that decide what a project's agent loop may do on this machine (ADR-006). Takes effect on
          the next check-in.
        </p>
        <label className="row" style={{ cursor: "pointer" }}>
          <span>Allow projects to reach the internet from this machine</span>
          <input
            type="checkbox"
            checked={s.allow_internet}
            onChange={(e) => run("set_config", { key: "HIVE_ALLOW_INTERNET", value: e.target.checked ? "true" : "false" })}
          />
        </label>
        <p className="muted" style={{ fontSize: 12 }}>Off by default. A card never gets network it didn't declare, even when this is on.</p>
        <label className="field">Tools</label>
        <select
          value={s.tools_level}
          onChange={(e) => run("set_config", { key: "HIVE_TOOLS_LEVEL", value: e.target.value })}
        >
          <option value="sandboxed_tools">Sandboxed tools (recommended) — scratch folder + sandboxed code execution</option>
          <option value="inference_only">Inference only — model in, tokens out, nothing else</option>
        </select>
      </div>
    </>
  );
}
