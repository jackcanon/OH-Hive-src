"use client";

import { useEffect, useRef, useState } from "react";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav, RequireMember, honey } from "@/components/RequireMember";

// Chat-first (Jack, 2026-09-12): "walk back from having the coordinator interview -- let people
// pick between straight agent chat and using the coordinator to build a project, it needs to feel
// more like a typical claude chat experience." This page used to drop every visitor straight into
// the interview (project-planning) system prompt; now it's a plain chat by default (mode "chat" --
// see supabase/functions/interview/index.ts), and "Turn this into a project" re-sends the SAME
// conversation with mode "plan" so nothing already said gets lost -- it just starts steering
// toward a buildable plan, same as the old interview did.
//
// This also drops the old local-first fallback (a text card on the hub's "Interviews" project,
// polled every ~3s) for this page specifically -- that polling cadence reads as broken for a
// freeform chat. Both modes now always go through the provider Edge Function (member's own key,
// else the hub's, charged to purchased/grant Honey) -- same cost model either way. The local text
// pool and its RPCs (hive_interview_send/poll) are untouched in the database if we want them back.

type Msg = { role: "user" | "assistant"; content: string };
type EdgeReply = { reply: string; project_id?: string; cards?: number; charged: number; balance: number | null; error?: string; detail?: string; brain?: string };
type Provider = { spendable_honey: number; budget_usd_cap: number | null; budget_usd_spent: number | null };
type Config = { byo: Record<string, { last4: string }>; provider: Provider | null };

function Chat() {
  const [msgs, setMsgs] = useState<Msg[]>([]);
  const [input, setInput] = useState("");
  const [pending, setPending] = useState(false);
  const [mode, setMode] = useState<"chat" | "plan">("chat");
  const [spent, setSpent] = useState(0);
  const [balance, setBalance] = useState<number | null>(null);
  const [cfg, setCfg] = useState<Config | null>(null);
  const [done, setDone] = useState<{ project_id: string; cards: number } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const bottom = useRef<HTMLDivElement>(null);

  useEffect(() => { if (msgs.length > 0 || pending) bottom.current?.scrollIntoView({ behavior: "smooth", block: "nearest" }); }, [msgs, pending]);
  useEffect(() => {
    supabaseBrowser().rpc("hive_interview_config").then(({ data }) => { if (data) setCfg(data as Config); });
  }, []);

  const provider = cfg?.provider ?? null;
  const providerOk = provider != null && Number(provider.spendable_honey) > 0 &&
    !(provider.budget_usd_cap != null && Number(provider.budget_usd_spent ?? 0) >= Number(provider.budget_usd_cap));
  const byo = cfg != null && Object.keys(cfg.byo ?? {}).length > 0;
  const cloudOk = byo || providerOk;
  const noPath = cfg != null && !cloudOk;

  async function post(nextMsgs: Msg[], turnMode: "chat" | "plan") {
    setPending(true);
    const { data, error } = await supabaseBrowser().functions.invoke<EdgeReply>("interview", {
      body: { messages: nextMsgs, mode: turnMode },
    });
    setPending(false);
    if (error || !data) { setErr(error?.message ?? "no response"); return; }
    if (data.error) { setErr(`${data.error}${data.detail ? `: ${data.detail}` : ""}`); return; }
    if (data.reply) setMsgs([...nextMsgs, { role: "assistant", content: data.reply }]);
    setSpent((s) => s + (data.charged ?? 0));
    if (data.balance != null) setBalance(data.balance);
    if (data.project_id) setDone({ project_id: data.project_id, cards: data.cards ?? 0 });
  }

  async function send() {
    const text = input.trim();
    if (!text || pending || noPath) return;
    setErr(null); setInput("");
    const next = [...msgs, { role: "user" as const, content: text }];
    setMsgs(next);
    await post(next, mode);
  }

  // Switches the SAME conversation over to plan mode instead of starting a fresh one -- the model
  // sees everything already said and starts working toward a buildable plan from there.
  async function buildProject() {
    if (mode !== "chat" || pending || noPath) return;
    setErr(null);
    setMode("plan");
    const next = [...msgs, { role: "user" as const, content: "Let's turn this into a project." }];
    setMsgs(next);
    await post(next, "plan");
  }

  return (
    <main style={{ maxWidth: 720, margin: "0 auto", padding: 24, display: "flex", flexDirection: "column", minHeight: "calc(100vh - 48px)" }}>
      <h1 style={{ margin: "8px 0 4px" }}>Chat</h1>
      <p style={{ color: "var(--muted-strong)", marginTop: 0 }}>
        {mode === "chat"
          ? "Ask anything, or think something through — when you're ready to build something on Hive, turn the conversation into a project."
          : "Building this into a project — Hive will ask anything it still needs to know."}
        {balance != null && <> · Wallet {honey(balance)}</>}{spent > 0 && <> · this chat has cost {honey(spent)}</>}
        {cfg != null && !byo && <> · <a href="/settings#keys" style={{ color: "var(--muted)" }}>use your own key</a></>}
      </p>
      {noPath && (
        <p style={{ background: "var(--warn-bg)", border: "1px solid var(--warn-border)", borderRadius: 8, padding: "10px 12px", fontSize: 13, color: "var(--warn-fg)" }}>
          Chat needs purchased Honey or <a href="/settings#keys">your own API key</a> right now — add one in Settings to start.
        </p>
      )}

      <div style={{ flex: 1, overflowY: "auto", display: "flex", flexDirection: "column", gap: 10, padding: "8px 0" }}>
        {msgs.length === 0 && (
          <div style={{ color: "var(--muted)", fontSize: 14 }}>
            Try: “Help me think through a name for my film festival” — or describe something you want made, like
            “A 60-second radio spot: script and three taglines.” You can turn either kind of conversation into a project once it's taken shape.
          </div>
        )}
        {msgs.map((m, i) => (
          <div key={i} style={{ alignSelf: m.role === "user" ? "flex-end" : "flex-start", maxWidth: "85%",
              background: m.role === "user" ? "var(--user-bubble)" : "var(--surface)", border: "1px solid var(--border)", borderRadius: 10, padding: "10px 14px", whiteSpace: "pre-wrap" }}>
            {m.content}
          </div>
        ))}
        {pending && <div style={{ color: "var(--muted)", fontSize: 13 }}>thinking…</div>}
        {!done && !pending && mode === "chat" && msgs.length >= 2 && (
          <button
            onClick={buildProject}
            disabled={noPath}
            style={{ alignSelf: "flex-start", fontSize: 13, padding: "6px 12px", cursor: noPath ? "default" : "pointer" }}
          >
            Turn this into a project →
          </button>
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
          <input value={input} onChange={(e) => setInput(e.target.value)} placeholder="Ask anything…" disabled={pending || noPath}
                 style={{ flex: 1, padding: 10, fontSize: 15 }} autoFocus />
          <button type="submit" disabled={pending || noPath || !input.trim()} style={{ padding: "10px 16px", cursor: "pointer" }}>Send</button>
        </form>
      )}
    </main>
  );
}

export default function NewProject() {
  return <RequireMember next="/new">{() => (<><Nav /><Chat /></>)}</RequireMember>;
}
