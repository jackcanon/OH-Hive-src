import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Snapshot } from "./App";

const gb = (b: number) => (b / 1073741824).toFixed(b > 10 * 1073741824 ? 0 : 2) + " GB";

function suggestedName(s: Snapshot): string {
  const raw = s.summary?.node?.display_name ?? "";
  return raw.toLowerCase().replace(/[^a-z0-9-]/g, "").slice(0, 24);
}

/** Regional-server role, in-process (hive_server::serve). Disk + uptime, not GPU. */
export function Server({ s, run, busy }: { s: Snapshot; run: (c: string, a?: Record<string, unknown>) => Promise<void>; busy: boolean }) {
  const sv = s.server;
  const tn = s.tunnel;
  const [url, setUrl] = useState(sv.public_url);
  const [gbOffered, setGb] = useState(sv.storage_gb);
  const [tier, setTier] = useState(sv.tier);
  const [name, setName] = useState(suggestedName(s));
  const host = (() => { try { return new URL(url).host; } catch { return null; } })();
  const hostname = name ? `${name}.ohghive.com` : "";

  if (!s.paired) return <div className="card"><p className="muted" style={{ margin: 0 }}>Pair this machine first (Node section).</p></div>;

  return (
    <>
      <div className="card">
        <div className="row">
          <div>
            <div style={{ fontWeight: 600 }}><span className={"dot" + (sv.running ? (sv.registered ? " on" : " busy") : "")} />{sv.running ? (sv.registered ? "Serving the Hive" : "Starting…") : "Regional server off"}</div>
            <div className="muted" style={{ fontSize: 12 }}>
              {sv.running ? <>{sv.coordinator ? "coordinator of the Hive" : sv.coordinator_name ? `follower · coordinator is ${sv.coordinator_name}` : "follower"} · {sv.blobs} blob{sv.blobs === 1 ? "" : "s"}, {gb(sv.used_bytes)}</> : "Holds artifacts, relays live boards, competes for coordinator. Earns Honey for bytes stored and served."}
            </div>
          </div>
          {sv.running
            ? <button className="danger" disabled={busy} onClick={() => run("server_stop")}>Stop</button>
            : <button className="primary" disabled={busy || !host} onClick={() => run("server_start", { publicUrl: url, storageGb: gbOffered, tier })}>Start serving</button>}
        </div>
        {sv.last_backup && <p className="muted" style={{ fontSize: 12, margin: "8px 0 0" }}>Last hub backup taken here: {sv.last_backup.slice(0, 12)}…</p>}
      </div>

      <div className="card">
        <h2>Reachability</h2>
        {!tn.available && (
          <>
            <label className="field">Public URL (how members and other servers reach this machine)</label>
            <input placeholder="https://yourname.ohghive.com" value={url} onChange={(e) => setUrl(e.target.value)} disabled={sv.running} />
            <p className="muted" style={{ fontSize: 12 }}>
              This build has no bundled Cloudflare Tunnel — set one up in Terminal: <code style={{ fontSize: 11 }}>cloudflared tunnel login</code>, <code style={{ fontSize: 11 }}>cloudflared tunnel create &lt;name&gt;</code>, <code style={{ fontSize: 11 }}>cloudflared tunnel route dns &lt;name&gt; &lt;name&gt;.ohghive.com</code>, point it at <code style={{ fontSize: 11 }}>localhost:8790</code>, then paste the hostname above.{" "}
              <a href="#" onClick={(e) => { e.preventDefault(); openUrl("https://github.com/jackcanon/ohhive-releases/blob/main/README.md"); }}>Guide</a>
            </p>
          </>
        )}

        {tn.available && !tn.logged_in && (
          <>
            <p className="muted" style={{ fontSize: 13, marginTop: 0 }}>
              A free Cloudflare Tunnel gives this machine a public HTTPS address — no port forwarding, no certificates. Connect your Cloudflare
              account and the app does the rest.
            </p>
            <button className="primary" disabled={busy} onClick={() => run("tunnel_login")}>Connect Cloudflare</button>
            <p className="muted" style={{ fontSize: 12 }}>Opens in your browser. Come back here once you've signed in.</p>
          </>
        )}

        {tn.available && tn.logged_in && !tn.hostname && (
          <>
            <p className="muted" style={{ fontSize: 13, marginTop: 0 }}>Cloudflare connected. Pick a short name for this machine — it becomes its address.</p>
            <label className="field">Name</label>
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <input value={name} onChange={(e) => setName(e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, ""))} placeholder="vanaheim" style={{ flex: 1 }} />
              <span className="muted" style={{ fontSize: 13 }}>.ohghive.com</span>
            </div>
            <button className="primary" disabled={busy || !name} style={{ marginTop: 10 }}
              onClick={() => run("tunnel_setup", { name, hostname }).then(() => setUrl(`https://${hostname}`))}>
              Create tunnel
            </button>
          </>
        )}

        {tn.available && tn.hostname && (
          <>
            <div className="row">
              <span><span className={"dot" + (tn.running ? " on" : "")} /><code style={{ fontSize: 13 }}>https://{tn.hostname}</code></span>
              <span className="muted" style={{ fontSize: 12 }}>{tn.running ? "connected" : sv.running ? "connecting…" : "starts with the server"}</span>
            </div>
            <p className="muted" style={{ fontSize: 12 }}>This is what members and other servers use to reach this machine.</p>
          </>
        )}
      </div>

      <div className="card">
        <h2>Offer</h2>
        <label className="field">Disk to offer: {gbOffered} GB</label>
        <input type="range" min={20} max={4000} step={10} value={gbOffered} onChange={(e) => setGb(Number(e.target.value))} disabled={sv.running} />
        <label className="field">Tier</label>
        <select value={tier} onChange={(e) => setTier(e.target.value)} disabled={sv.running}>
          <option value="primary">Primary — always on, first choice for relay and storage</option>
          <option value="standby">Standby — backups and overflow only</option>
        </select>
        <p className="muted" style={{ fontSize: 12 }}>Listening on {sv.listen}. Blobs live in <code style={{ fontSize: 11 }}>{sv.data_dir}</code>. Operator: {sv.operator}.</p>
      </div>
    </>
  );
}
