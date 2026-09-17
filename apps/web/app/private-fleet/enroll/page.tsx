"use client";

import { useEffect, useState } from "react";
import type { Session } from "@supabase/supabase-js";
import { parseDesktopBridge, desktopCallback, type DesktopBridge } from "./desktop-bridge";
import { supabaseBrowser } from "@/lib/supabase";

type Fleet = { id: string; name: string };
type Challenge = { authority_id: string; node_id: string; credential_sha256: string; nonce: string; expires_at: number };

export default function PrivateFleetEnrollment() {
  const [desktop, setDesktop] = useState<DesktopBridge | null>(null);
  const [fleetLoading, setFleetLoading] = useState(true);
  const [fleetLoadFailed, setFleetLoadFailed] = useState(false);
  const [returned, setReturned] = useState(false);
  const [session, setSession] = useState<Session | null>(null);
  const [loading, setLoading] = useState(true);
  const [fleets, setFleets] = useState<Fleet[]>([]);
  const [fleet, setFleet] = useState("");
  const [name, setName] = useState("My Private Fleet");
  const [request, setRequest] = useState("");
  const [review, setReview] = useState<Challenge | null>(null);
  const [approval, setApproval] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");

  useEffect(() => {
    const key = "hive-desktop-enrollment";
    try {
      const incoming = location.hash.startsWith("#desktop=") ? decodeURIComponent(location.hash.slice(9)) : null;
      const encoded = incoming ?? sessionStorage.getItem(key);
      const bridge = encoded ? parseDesktopBridge(encoded) : null;
      if (bridge) { setDesktop(bridge); sessionStorage.setItem(key, encoded!); }
      else { sessionStorage.removeItem(key); if (incoming) setMessage("This registration request expired. Return to the app and try again."); }
      if (incoming) history.replaceState(null, "", location.pathname + location.search);
    } catch { setMessage("Could not restore registration. Return to the app and try again."); }
  }, []);

  useEffect(() => {
    const sb = supabaseBrowser();
    sb.auth.getSession().then(({ data }) => { setSession(data.session); setLoading(false); });
    const { data } = sb.auth.onAuthStateChange((_event, value) => { setSession(value); });
    return () => data.subscription.unsubscribe();
  }, []);
  useEffect(() => {
    setReview(null); setApproval(""); setFleets([]); setFleet(""); setFleetLoading(true); setFleetLoadFailed(false);
    if (!session) return;
    let active = true;
    supabaseBrowser().schema("hive").from("private_fleets").select("id,name").order("created_at").then(({ data, error }) => {
      if (!active) return;
      setFleetLoading(false);
      if (error) { setFleetLoadFailed(true); setMessage("Private Fleet sign-in is not available yet. Please reload to try again."); }
      else { setFleets(data ?? []); setFleet(data?.[0]?.id ?? ""); }
    });
    return () => { active = false; };
  }, [session?.user.id]);

  async function signIn(provider: "apple" | "google") {
    const { error } = await supabaseBrowser().auth.signInWithOAuth({ provider, options: { redirectTo: `${location.origin}/auth/callback?next=/private-fleet/enroll` } });
    if (error) setMessage("Sign-in could not start. Please try again.");
  }
  async function invoke(body: object) {
    const { data, error } = await supabaseBrowser().functions.invoke("private-fleet-enroll", { body });
    if (error || data?.error) throw new Error("We couldn't complete that request. Check your sign-in and try a fresh connection request.");
    return data;
  }
  async function createFleet() {
    setBusy(true); setMessage("");
    try { const data = await invoke({ action: "create_fleet", name }); setFleets(previous => [...previous, data.fleet]); setFleet(data.fleet.id); }
    catch (error) { setMessage((error as Error).message); }
    finally { setBusy(false); }
  }
  function reviewRequest() {
    setMessage(""); setApproval(""); setReview(null);
    try {
      const c = JSON.parse(request) as Challenge;
      if (!c || typeof c.authority_id !== "string" || typeof c.node_id !== "string" || typeof c.credential_sha256 !== "string" || !/^[a-f0-9]{64}$/.test(c.credential_sha256) || !Number.isSafeInteger(c.expires_at) || c.expires_at <= Date.now() / 1000) throw new Error();
      setReview(c);
    } catch { setMessage("Paste a fresh connection request from Hive on the computer you want to add."); }
  }
  async function approve() {
    if (!review || !fleet) return;
    setBusy(true); setMessage("");
    try { const data = await invoke({ action: "approve", fleet_id: fleet, challenge: review }); setApproval(JSON.stringify(data.assertion)); setReview(null); setRequest(""); }
    catch (error) { setMessage((error as Error).message); }
    finally { setBusy(false); }
  }
  async function registerDesktop() {
    if (!desktop || !session || busy || fleetLoading || fleetLoadFailed) return;
    setBusy(true); setMessage("");
    try {
      if (!parseDesktopBridge(btoa(JSON.stringify(desktop)))) throw new Error("This request expired. Return to the app and try again.");
      let target = fleet;
      if (!target && fleets.length === 0) {
        const created = await invoke({ action: "create_fleet", name: "My Private Fleet" });
        target = created.fleet.id;
        setFleets([created.fleet]); setFleet(target);
      }
      if (!target) throw new Error("Choose your fleet first.");
      const data = await invoke({ action: "approve", fleet_id: target, challenge: JSON.parse(desktop.request) });
      const callback = desktopCallback(desktop, data.assertion);
      sessionStorage.removeItem("hive-desktop-enrollment");
      setReturned(true);
      location.assign(callback);
    } catch (error) { setMessage((error as Error).message); }
    finally { setBusy(false); }
  }
  if (desktop) return <main style={{ maxWidth: 520, margin: "64px auto", padding: 24, lineHeight: 1.6 }}>
    <h1>{returned ? "Return to Loki’s Den" : "Register your computer"}</h1>
    <p>{returned ? "The app will confirm when registration is complete. If it did not return, start sign-in again in the app." : "Sign in, then approve the computer where you just opened Loki’s Den."}</p>
    {message && <p role="alert">{message}</p>}
    {!returned && (loading ? <p>Checking sign-in…</p> : !session ? <>
      <button onClick={() => signIn("google")}>Continue with Google</button>{" "}
      <button onClick={() => signIn("apple")}>Continue with Apple</button>
    </> : <>
      <p>Signed in as {session.user.email ?? "your account"}.</p>
      {fleets.length > 1 && <label>Your fleet <select value={fleet} disabled={busy} onChange={e => setFleet(e.target.value)}>{fleets.map(f => <option key={f.id} value={f.id}>{f.name}</option>)}</select></label>}
      <p>This registers the computer with your private projects and agents. Only continue if you started this request in your app.</p>
      <button disabled={busy || fleetLoading || fleetLoadFailed} onClick={registerDesktop}>{busy ? "Registering…" : "Approve this computer"}</button>
      <details><summary>Computer details</summary><p>Fingerprint: {JSON.parse(desktop.request).credential_sha256.slice(0,12)}</p></details>
    </>)}
  </main>;
  return <main style={{ maxWidth: 640, margin: "48px auto", padding: 24, lineHeight: 1.6 }}>
    <h1>Connect your Private Fleet</h1>
    <p>Bring your own computers and agents together. You can set up a Private Fleet without an OHG community invitation.</p>
    {message && <p role="alert">{message}</p>}
    {loading ? <p>Checking sign-in…</p> : !session ? <>
      <p>Sign in to establish your Hive identity.</p>
      <button onClick={() => signIn("google")}>Continue with Google</button>{" "}
      <button onClick={() => signIn("apple")}>Continue with Apple</button>
    </> : <>
      <p>Signed in as {session.user.email ?? "your Hive account"}.</p>
      <label>Choose your fleet <select value={fleet} disabled={busy} onChange={e => { setFleet(e.target.value); setApproval(""); }}>
        <option value="">Choose a fleet</option>{fleets.map(f => <option key={f.id} value={f.id}>{f.name}</option>)}
      </select></label>
      <p><label>New fleet name <input value={name} maxLength={80} disabled={busy} onChange={e => setName(e.target.value)} /></label>{" "}
        <button disabled={busy || !name.trim()} onClick={createFleet}>Create fleet</button></p>
      {!approval && <>
        <label>Connection request from Hive<textarea value={request} rows={4} style={{ width: "100%" }} disabled={busy} onChange={e => { setRequest(e.target.value); setReview(null); }} /></label>
        <button disabled={busy || !request || !fleet} onClick={reviewRequest}>Review computer</button>
      </>}
      {review && <section aria-label="Review this computer">
        <h2>Approve this computer?</h2>
        <p>Compare these details with Hive on the computer you are adding.</p>
        <p>Computer fingerprint: <strong>{review.credential_sha256.slice(0, 12)}</strong><br />Primary identifier: <strong>{review.authority_id}</strong></p>
        <p>This grants the computer access to your fleet’s Bots conversations and agents. Only approve a request you just started.</p>
        <button disabled={busy} onClick={approve}>Approve computer</button>{" "}<button disabled={busy} onClick={() => setReview(null)}>Cancel</button>
      </section>}
      {approval && <section aria-label="Enrollment approval">
        <h2>Return to Hive</h2><p>Copy this approval into Hive on the same computer. It expires within five minutes. The computer is connected only after Hive accepts it.</p>
        <textarea aria-label="Approval to copy into Hive" readOnly rows={4} value={approval} style={{ width: "100%" }} />
        <button onClick={async () => { try { await navigator.clipboard.writeText(approval); setMessage("Approval copied. Return to Hive to finish."); } catch { setMessage("Select the approval above and copy it manually."); } }}>Copy approval</button>
      </section>}
    </>}
  </main>;
}
