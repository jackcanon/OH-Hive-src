"use client";

import { useEffect, useRef, useState } from "react";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav, RequireMember, honey } from "@/components/RequireMember";

// Chat reverts to local-first (Jack, 2026-09-12): "Let's revert the chat to local, and they can
// input their own api for claude or nous or chatgpt." Two paths now, chosen automatically per
// member -- no hub-funded cloud fallback anymore (see supabase/functions/interview/index.ts):
//   - No key of your own: the Hive's local community-compute text pool runs the conversation
//     (hive_interview_send/poll -- a text card on the hub's "Interviews" project, picked up by an
//     idle member node, polled every ~3s). Free -- nothing is ever charged to your wallet.
//   - Your own Anthropic/OpenAI/Nous key on file (Settings -> AI key): the `interview` Edge
//     Function calls straight out to that provider instead. Also free to the Hive -- your key,
//     your bill, one request/response per turn instead of a poll loop.
// Whichever path is active, "Turn this into a project" re-sends the SAME conversation with
// mode:"plan" instead of starting fresh -- the model has everything already said and starts
// working toward a buildable plan from there, exactly as before the local/cloud split.
//
// The local model can't tool-call, so its plan comes back as text with a PLAN marker and a fenced
// ```json block (hive.interview_prompt, unchanged since before this change) -- splitPlan() below
// strips that out of what's shown and hands the parsed object to hive_interview_plan() to actually
// materialize the project client-side. The cloud path calls create_project_plan as a real tool and
// materializes server-side (hive.create_project_from_plan) -- splitPlan there is just a display
// nicety in case a model narrates JSON anyway.

type Msg = { role: "user" | "assistant"; content: string };
type EdgeReply = { reply: string; project_id?: string; cards?: number; charged: number; balance: number | null; error?: string; detail?: string; brain?: string };
type Config = { byo: Record<string, { last4: string }>; nodes_online: number };
type PollResult = {
  session_id: string; status: string; mode: "chat" | "plan"; messages: Msg[]; pending: boolean;
  project_id?: string | null; nodes_online: number; balance: number | null; error?: string;
};

// The local model embeds a plan as summary text + PLAN + a fenced ```json block (or, failing that,
// a trailing bare object) once it has enough -- everything before that marker is the
// human-readable summary to actually show the member.
function splitPlan(content: string): { text: string; plan: Record<string, unknown> | null } {
  const patterns: RegExp[] = [/```json\s*([\s\S]*?)```/i, /(\{[\s\S]*"schema_version"[\s\S]*\})\s*$/];
  for (const re of patterns) {
    const m = content.match(re);
    if (!m || m.index == null) continue;
    try {
      const plan = JSON.parse(m[1]);
      const text = content.slice(0, m.index).replace(/PLAN\s*$/, "").trim();
      return { text: text || "Here's the plan.", plan };
    } catch {
      // not valid JSON after all -- try the next pattern rather than giving up
    }
  }
  return { text: content, plan: null };
}

function Chat() {
  const [msgs, setMsgs] = useState<Msg[]>([]);
  const [input, setInput] = useState("");
  const [pending, setPending] = useState(false);
  const [mode, setMode] = useState<"chat" | "plan">("chat");
  const [spent, setSpent] = useState(0);
  const [balance, setBalance] = useState<number | null>(null);
  const [cfg, setCfg] = useState<Config | null>(null);
  const [session, setSession] = useState<string | null>(null);
  const [nodesOnline, setNodesOnline] = useState<number | null>(null);
  const [done, setDone] = useState<{ project_id: string; cards: number } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const bottom = useRef<HTMLDivElement>(null);

  useEffect(() => { if (msgs.length > 0 || pending) bottom.current?.scrollIntoView({ behavior: "smooth", block: "nearest" }); }, [msgs, pending]);
  useEffect(() => {
    supabaseBrowser().rpc("hive_interview_config").then(({ data }) => {
      if (!data) return;
      const c = data as Config;
      setCfg(c);
      setNodesOnline(c.nodes_online);
    });
  }, []);

  const byo = cfg != null && Object.keys(cfg.byo ?? {}).length > 0;
  const noPath = cfg != null && !byo && nodesOnline === 0;

  // Cloud path: one request/response through the member's own key.
  async function postCloud(nextMsgs: Msg[], turnMode: "chat" | "plan") {
    const { data, error } = await supabaseBrowser().functions.invoke<EdgeReply>("interview", {
      body: { messages: nextMsgs, mode: turnMode },
    });
    if (error || !data) { setErr(error?.message ?? "no response"); return; }
    if (data.error) { setErr(`${data.error}${data.detail ? `: ${data.detail}` : ""}`); return; }
    if (data.reply) {
      const { text } = splitPlan(data.reply);
      setMsgs([...nextMsgs, { role: "assistant", content: text }]);
    }
    setSpent((s) => s + (data.charged ?? 0));
    if (data.balance != null) setBalance(data.balance);
    if (data.project_id) setDone({ project_id: data.project_id, cards: data.cards ?? 0 });
  }

  // Local path: send to the community text pool, then poll until a node has picked it up and
  // replied (~3s cadence -- these run on idle member machines, not a hosted API).
  async function pollUntilDone(sid: string): Promise<PollResult | null> {
    for (;;) {
      await new Promise((r) => setTimeout(r, 3000));
      const { data, error } = await supabaseBrowser().rpc("hive_interview_poll", { p_session: sid });
      if (error) { setErr(error.message); return null; }
      const res = data as PollResult;
      if (res.error) { setErr(res.error); return res; }
      if (!res.pending) return res;
    }
  }

  async function postLocal(text: string, turnMode: "chat" | "plan") {
    const { data, error } = await supabaseBrowser().rpc("hive_interview_send", { p_session: session, p_text: text, p_mode: turnMode });
    if (error) { setErr(error.message); return; }
    const sent = data as { session_id: string; mode: "chat" | "plan" };
    setSession(sent.session_id);
    setMode(sent.mode);
    const res = await pollUntilDone(sent.session_id);
    if (!res) return;
    setMode(res.mode);
    setNodesOnline(res.nodes_online);
    if (res.balance != null) setBalance(res.balance);
    const last = res.messages[res.messages.length - 1];
    if (last?.role !== "assistant") { setMsgs(res.messages); return; }
    const { text: shown, plan } = splitPlan(last.content);
    setMsgs(res.messages.slice(0, -1).concat({ role: "assistant", content: shown }));
    if (plan) {
      const { data: created, error: cerr } = await supabaseBrowser().rpc("hive_interview_plan", { p_session: sent.session_id, p_plan: plan });
      if (cerr) { setErr(`plan_rejected: ${cerr.message}`); return; }
      const c = created as { project_id: string; cards: number };
      setDone({ project_id: c.project_id, cards: c.cards ?? 0 });
    }
  }

  async function send() {
    const text = input.trim();
    if (!text || pending || noPath) return;
    setErr(null); setInput("");
    const next = [...msgs, { role: "user" as const, content: text }];
    setMsgs(next);
    setPending(true);
    if (byo) await postCloud(next, mode); else await postLocal(text, mode);
    setPending(false);
  }

  // Switches the SAME conversation over to plan mode instead of starting a fresh one -- the model
  // sees everything already said and starts working toward a buildable plan from there.
  async function buildProject() {
    if (mode !== "chat" || pending || noPath) return;
    setErr(null);
    setMode("plan");
    const askText = "Let's turn this into a project.";
    const next = [...msgs, { role: "user" as const, content: askText }];
    setMsgs(next);
    setPending(true);
    if (byo) await postCloud(next, "plan"); else await postLocal(askText, "plan");
    setPending(false);
  }

  return (
    <main style={{ maxWidth: 720, margin: "0 auto", padding: 24, display: "flex", flexDirection: "column", minHeight: "calc(100vh - 48px)" }}>
      <h1 style={{ margin: "8px 0 4px" }}>Chat</h1>
      <p style={{ color: "var(--muted-strong)", marginTop: 0 }}>
        {mode === "chat"
          ? "Ask anything, or think something through — when you're ready to build something on Hive, turn the conversation into a project."
          : "Building this into a project — Hive will ask anything it still needs to know."}
        {balance != null && <> · Wallet {honey(balance)}</>}{spent > 0 && <> · this chat has cost {honey(spent)}</>}
        {cfg != null && !byo && <> · running on the Hive's local nodes{nodesOnline != null && <> ({nodesOnline} online)</>} · <a href="/settings#keys" style={{ color: "var(--muted)" }}>use your own key instead</a></>}
        {byo && <> · using your own key</>}
      </p>
      {noPath && (
        <p style={{ background: "var(--warn-bg)", border: "1px solid var(--warn-border)", borderRadius: 8, padding: "10px 12px", fontSize: 13, color: "var(--warn-fg)" }}>
          No local node is online right now, and you don't have <a href="/settings#keys">your own API key</a> on file — add one in Settings, or try again once a node checks in.
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
        {pending && <div style={{ color: "var(--muted)", fontSize: 13 }}>{byo ? "thinking…" : "waiting on a Hive node…"}</div>}
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
