"use client";

import { useEffect, useRef, useState } from "react";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav, RequireMember, honey } from "@/components/RequireMember";

// Provider-first since 2026-09-06 (Jack: the local 12B's follow-ups were too weak for members):
// the Edge Function runs a frontier model — the member's own Anthropic/OpenAI key if they stored one
// (free to the Hive), else the hub's Anthropic key (charged from purchased/grant Honey under
// provider_budget). The local text pool (a text card on the Hive's "Interviews" project, paid at the
// local rate with earned Honey) is the fallback when neither is available. hive.settings.interview_mode
// can flip it back to local_first.

type Msg = { role: "user" | "assistant"; content: string; cost?: number };
type Poll = { session_id: string; status: string; messages: Msg[]; pending: boolean; pending_card_status?: string | null;
              project_id?: string | null; nodes_online: number; balance: number; error?: string };
type EdgeReply = { reply: string; project_id?: string; cards?: number; charged: number; balance: number | null; error?: string; detail?: string; brain?: string };
type Provider = { spendable_honey: number; budget_usd_cap: number | null; budget_usd_spent: number | null };
type Config = { mode: "provider_first" | "local_first"; web_search: boolean; byo: Record<string, { last4: string }>; provider: Provider | null; nodes_online: number; local_model: string | null };

/** Pull the plan JSON out of an interviewer reply: text before PLAN is the human summary. */
function splitPlan(content: string): { text: string; plan: unknown | null } {
  const m = content.match(/```json\s*([\s\S]*?)```/i) ?? content.match(/(\{[\s\S]*"schema_version"[\s\S]*\})\s*$/);
  if (!m) return { text: content, plan: null };
  try {
    const plan = JSON.parse(m[1]);
    const text = content.slice(0, m.index).replace(/\n?PLAN\s*$/i, "").trim();
    return { text: text || "Here's the plan.", plan };
  } catch { return { text: content, plan: null }; }
}

function Interview() {
  const [session, setSession] = useState<string | null>(null);
  const [msgs, setMsgs] = useState<Msg[]>([]);
  const [input, setInput] = useState("");
  const [pending, setPending] = useState(false);
  const [pendingStatus, setPendingStatus] = useState<string | null>(null);
  const [spent, setSpent] = useState(0);
  const [balance, setBalance] = useState<number | null>(null);
  const [nodesOnline, setNodesOnline] = useState<number | null>(null);
  const [provider, setProvider] = useState<Provider | null>(null);
  const [cfg, setCfg] = useState<Config | null>(null);
  const [brain, setBrain] = useState<string | null>(null);
  const [done, setDone] = useState<{ project_id: string; cards: number } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const bottom = useRef<HTMLDivElement>(null);
  const planned = useRef(false);

  useEffect(() => { bottom.current?.scrollIntoView({ behavior: "smooth" }); }, [msgs, pending]);
  useEffect(() => {
    const sb = supabaseBrowser();
    sb.rpc("hive_interview_config").then(({ data }) => {
      if (!data) return;
      const c = data as Config;
      setCfg(c); setProvider(c.provider); setNodesOnline(c.nodes_online);
    });
  }, []);

  const providerOk = provider != null && Number(provider.spendable_honey) > 0 &&
    !(provider.budget_usd_cap != null && Number(provider.budget_usd_spent ?? 0) >= Number(provider.budget_usd_cap));
  const byo = cfg != null && Object.keys(cfg.byo ?? {}).length > 0;
  const cloudOk = byo || providerOk;
  // provider_first: cloud whenever it's possible; local_first: cloud only when no node is up.
  const useLocal = cfg == null ? true : cfg.mode === "local_first" ? (nodesOnline == null || nodesOnline > 0 || !cloudOk) : !cloudOk;

  async function tryPlan(sid: string, content: string) {
    const { plan } = splitPlan(content);
    if (!plan || planned.current) return;
    planned.current = true;
    const { data, error } = await supabaseBrowser().rpc("hive_interview_plan", { p_session: sid, p_plan: plan });
    if (error) { setErr(`The plan came back but couldn't be created: ${error.message.replace(/_/g, " ")}. Ask the interviewer to fix it.`); planned.current = false; return; }
    const d = data as { project_id: string; cards?: number };
    setDone({ project_id: d.project_id, cards: d.cards ?? 0 });
  }

  // Poll the session while a turn is in flight.
  useEffect(() => {
    if (!session || !pending) return;
    let stop = false;
    const tick = async () => {
      const { data, error } = await supabaseBrowser().rpc("hive_interview_poll", { p_session: session });
      if (stop) return;
      if (error) { setErr(error.message); setPending(false); return; }
      const p = data as Poll;
      setBalance(p.balance);
      setNodesOnline(p.nodes_online);
      setPendingStatus(p.pending_card_status ?? null);
      if (p.error === "turn_failed") { setErr("That turn failed on the node. Try sending again."); setPending(false); return; }
      if (!p.pending) {
        const shown = p.messages.map((m) => m.role === "assistant" ? { ...m, content: splitPlan(m.content).text } : m);
        setMsgs(shown);
        setSpent(p.messages.reduce((s, m) => s + Number(m.cost ?? 0), 0));
        setPending(false);
        const last = p.messages[p.messages.length - 1];
        if (last?.role === "assistant") await tryPlan(p.session_id, last.content);
      }
    };
    tick();
    const t = setInterval(tick, 3000);
    return () => { stop = true; clearInterval(t); };
  }, [session, pending]);

  async function send() {
    const text = input.trim();
    if (!text || pending) return;
    setErr(null); setInput("");
    const sb = supabaseBrowser();
    if (useLocal) {
      setMsgs((m) => [...m, { role: "user", content: text }]);
      const { data, error } = await sb.rpc("hive_interview_send", { p_session: session, p_text: text });
      if (error) { setErr(error.message.replace(/_/g, " ")); return; }
      const d = data as { session_id: string };
      setSession(d.session_id);
      setPending(true);
    } else {
      // Fallback: provider-backed Edge Function (purchased/grant honey only).
      const next = [...msgs, { role: "user" as const, content: text }];
      setMsgs(next); setPending(true);
      const { data, error } = await sb.functions.invoke<EdgeReply>("interview", { body: { messages: next.map(({ role, content }) => ({ role, content })) } });
      setPending(false);
      if (error || !data) { setErr(error?.message ?? "no response"); return; }
      if (data.error) setErr(`${data.error}${data.detail ? `: ${data.detail}` : ""}`);
      if (data.reply) setMsgs([...next, { role: "assistant", content: data.reply }]);
      if (data.brain) setBrain(data.brain);
      setSpent((s) => s + (data.charged ?? 0));
      if (data.balance != null) setBalance(data.balance);
      if (data.project_id) setDone({ project_id: data.project_id, cards: data.cards ?? 0 });
    }
  }

  const noPath = nodesOnline === 0 && !cloudOk;

  return (
    <main style={{ maxWidth: 720, margin: "0 auto", padding: 24, display: "flex", flexDirection: "column", minHeight: "calc(100vh - 48px)" }}>
      <h1 style={{ margin: "8px 0 4px" }}>Start a project</h1>
      <p style={{ color: "var(--muted-strong)", marginTop: 0 }}>
        Tell the interviewer what you want to make. It will ask a few questions, then build your kanban.
        {balance != null && <> · Wallet {honey(balance)}</>}{spent > 0 && <> · this interview {honey(spent)}</>}
        {cfg != null && <> · {useLocal ? `answered by the Hive's local model${cfg.local_model ? ` (${cfg.local_model})` : ""}, ${nodesOnline} node${nodesOnline === 1 ? "" : "s"} online` : brain ? `answered by ${brain}` : byo ? "answered with your own API key" : "answered by the hub's cloud model"}</>}
        {cfg != null && !useLocal && !byo && <> · <a href="/settings#keys" style={{ color: "var(--muted)" }}>use your own key</a></>}
      </p>
      {noPath && (
        <p style={{ background: "var(--warn-bg)", border: "1px solid var(--warn-border)", borderRadius: 8, padding: "10px 12px", fontSize: 13, color: "var(--warn-fg)" }}>
          No Hive node is online right now to run the interviewer, and the cloud interviewer needs purchased Honey or <a href="/settings#keys">your own API key</a>. Try again when a node is up, or pair one of your own machines.
        </p>
      )}

      <div style={{ flex: 1, overflowY: "auto", display: "flex", flexDirection: "column", gap: 10, padding: "8px 0" }}>
        {msgs.length === 0 && (
          <div style={{ color: "var(--muted)", fontSize: 14 }}>
            Try: “A 60-second radio spot for our film festival: script and three taglines. No internet needed. Owner-only.”
          </div>
        )}
        {msgs.map((m, i) => (
          <div key={i} style={{ alignSelf: m.role === "user" ? "flex-end" : "flex-start", maxWidth: "85%",
              background: m.role === "user" ? "var(--user-bubble)" : "var(--surface)", border: "1px solid var(--border)", borderRadius: 10, padding: "10px 14px", whiteSpace: "pre-wrap" }}>
            {m.content}
            {m.cost != null && Number(m.cost) > 0 && <div style={{ fontSize: 11, color: "var(--muted)", marginTop: 6 }}>{honey(Number(m.cost))}</div>}
          </div>
        ))}
        {pending && (
          <div style={{ color: "var(--muted)", fontSize: 13 }}>
            {pendingStatus === "running" ? "a node is thinking…" : pendingStatus === "ready" ? "waiting for a node to pick this up…" : "thinking…"}
          </div>
        )}
        {done && (
          <div style={{ border: "1px solid var(--ok)", borderRadius: 10, padding: 14, background: "var(--ok-bg)" }}>
            <strong>Project created</strong> with {done.cards} cards.{" "}
            <a href={`/projects/${done.project_id}`}>Open the board</a> — fund it from your wallet and nodes will start picking up cards.
          </div>
        )}
        {err && <div style={{ color: "var(--danger)", fontSize: 13 }}>{err}</div>}
        <div ref={bottom} />
      </div>

      {!done && (
        <form onSubmit={(e) => { e.preventDefault(); send(); }} style={{ display: "flex", gap: 8, paddingTop: 12, borderTop: "1px solid var(--border)" }}>
          <input value={input} onChange={(e) => setInput(e.target.value)} placeholder="What do you want to make?" disabled={pending || noPath}
                 style={{ flex: 1, padding: 10, fontSize: 15 }} autoFocus />
          <button type="submit" disabled={pending || noPath || !input.trim()} style={{ padding: "10px 16px", cursor: "pointer" }}>Send</button>
        </form>
      )}
    </main>
  );
}

export default function NewProject() {
  return <RequireMember next="/new">{() => (<><Nav /><Interview /></>)}</RequireMember>;
}
