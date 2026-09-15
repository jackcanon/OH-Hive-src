import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

type Account = { state: string; detail: string; email?: string; plan?: string; auth_url?: string; user_code?: string };

export function ChatGPTConnection() {
  const generation = useRef(0);
  const inFlight = useRef(false);
  const [status, setStatus] = useState<Account>();
  const [busy, setBusy] = useState(false);
  const [binary, setBinary] = useState("");
  const [error, setError] = useState("");
  useEffect(() => {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      const requestGeneration = generation.current;
      try {
        if (inFlight.current) return;
        const next = await invoke<Account>("chatgpt_account", { action: "status" });
        if (!stopped && requestGeneration === generation.current) setStatus(next);
      } catch { if (!stopped && requestGeneration === generation.current) setError("Could not reach the account service."); }
      finally { if (!stopped) timer = setTimeout(poll, 2000); }
    }
    void poll();
    return () => { stopped = true; clearTimeout(timer); };
  }, []);
  async function perform(action: string) {
    if (inFlight.current) return;
    inFlight.current = true;
    generation.current += 1;
    setBusy(true); setError("");
    try {
      const next = await invoke<Account>("chatgpt_account", { action, binary: binary || null });
      setStatus(next);
      if ((action === "connect" || action === "device") && next.auth_url) await openUrl(next.auth_url);
    } catch { setError("Could not complete this step. You can retry or reopen the sign-in page."); }
    finally { inFlight.current = false; setBusy(false); }
  }
  return <section className="card">
    <h2>Connect ChatGPT</h2>
    <p className="muted">Use your ChatGPT account with Hive. This preview connects your account; agent chat and fleet delegation are coming separately.</p>
    <p role="status">{status?.detail ?? "Connect your ChatGPT account to get started."}</p>
    {status?.email && <p>{status.email}{status.plan ? ` · ${status.plan}` : ""}</p>}
    {status?.user_code && <p>Sign-in code: <strong>{status.user_code}</strong></p>}
    {error && <p role="alert">{error}</p>}
    <div className="row">
      {status?.state === "signing_in" ? <>
        <button disabled={busy} onClick={() => { if (status.auth_url) void openUrl(status.auth_url).catch(() => setError("Could not open your browser.")); }}>Open sign-in page</button>
        <button disabled={busy} onClick={() => void perform("cancel")}>Cancel</button>
      </> : status?.state === "connected" ?
        <button disabled={busy} onClick={() => void perform("disconnect")}>Disconnect</button> : <>
        <button disabled={busy} onClick={() => void perform("connect")}>Connect ChatGPT</button>
        <button disabled={busy} onClick={() => void perform("device")}>Use device sign-in</button>
      </>}
      {busy && <span>Working…</span>}
    </div>
    <details><summary>Advanced</summary>
      <p>Requires the verified Codex 0.149.0 runtime. Hive looks for it automatically.</p>
      <label>Full path to Codex (optional)<input value={binary} onChange={e => setBinary(e.target.value)} /></label>
    </details>
  </section>;
}
