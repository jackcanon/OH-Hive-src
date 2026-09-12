"use client";

import { useEffect, useState } from "react";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav, RequireMember } from "@/components/RequireMember";
import { friendlyError } from "@/lib/errors";

type FeatureRequest = {
  id: string;
  title: string;
  description: string;
  status: "open" | "planned" | "in_progress" | "shipped" | "declined";
  created_at: string;
  submitted_by: string;
  votes: number;
  voted_by_me: boolean;
};

const STATUS_LABEL: Record<FeatureRequest["status"], string> = {
  open: "Open",
  planned: "Planned",
  in_progress: "In progress",
  shipped: "Shipped",
  declined: "Declined",
};
const STATUS_COLOR: Record<FeatureRequest["status"], string> = {
  open: "var(--muted-strong)",
  planned: "var(--accent)",
  in_progress: "var(--accent)",
  shipped: "var(--ok)",
  declined: "var(--muted)",
};

function RequestsView() {
  const [requests, setRequests] = useState<FeatureRequest[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [voting, setVoting] = useState<string | null>(null);
  const [isAdmin, setIsAdmin] = useState(false);
  const [statusBusy, setStatusBusy] = useState<string | null>(null);

  const load = () =>
    supabaseBrowser().rpc("hive_feature_request_list", {}).then(({ data, error }) => {
      if (error) setErr(friendlyError(error.message));
      else setRequests((data ?? []) as FeatureRequest[]);
    });

  useEffect(() => {
    load();
    supabaseBrowser().rpc("hive_am_i_admin", {}).then(({ data }) => setIsAdmin(!!data));
  }, []);

  // Admin-only (Jack, 2026-09-12): "planned" is the deliberate "yes, build this" signal a
  // scheduled agent run watches for -- vote count alone doesn't mean something's well-scoped.
  async function setStatus(r: FeatureRequest, status: FeatureRequest["status"]) {
    setStatusBusy(r.id);
    const { error } = await supabaseBrowser().rpc("hive_admin_feature_request_set_status", { p_request_id: r.id, p_status: status });
    setStatusBusy(null);
    if (error) { setErr(friendlyError(error.message)); return; }
    load();
  }

  const submit = async () => {
    if (title.trim().length < 3) { setErr("Give it a few more words — at least 3 characters."); return; }
    setSubmitting(true);
    setErr(null);
    const { error } = await supabaseBrowser().rpc("hive_feature_request_create", {
      p_title: title.trim(),
      p_description: description.trim(),
    });
    setSubmitting(false);
    if (error) { setErr(friendlyError(error.message)); return; }
    setTitle("");
    setDescription("");
    load();
  };

  const vote = async (r: FeatureRequest) => {
    setVoting(r.id);
    const { error } = await supabaseBrowser().rpc("hive_feature_request_vote", { p_request_id: r.id, p_on: !r.voted_by_me });
    setVoting(null);
    if (error) { setErr(friendlyError(error.message)); return; }
    load();
  };

  return (
    <main style={{ maxWidth: 720, margin: "0 auto", padding: 24 }}>
      <h1 style={{ margin: "8px 0" }}>Feature requests</h1>
      <p style={{ color: "var(--muted-strong)", fontSize: 14, margin: "0 0 24px" }}>
        Tell us what you want Hive to do next. Everyone can see and upvote what's been suggested.
      </p>

      <section style={{ border: "1px solid var(--border)", borderRadius: 10, padding: 16, marginBottom: 28, background: "var(--surface)" }}>
        <h2 style={{ fontSize: 15, margin: "0 0 10px" }}>Suggest something</h2>
        <input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="One line: what should Hive do?"
          maxLength={120}
          style={{ width: "100%", padding: "8px 10px", marginBottom: 8, fontSize: 14, borderRadius: 6, border: "1px solid var(--border)", background: "var(--bg)", color: "inherit" }}
        />
        <textarea
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="Anything that'd help us understand it — optional."
          maxLength={4000}
          rows={3}
          style={{ width: "100%", padding: "8px 10px", marginBottom: 10, fontSize: 14, borderRadius: 6, border: "1px solid var(--border)", background: "var(--bg)", color: "inherit", resize: "vertical" }}
        />
        <button
          onClick={submit}
          disabled={submitting || title.trim().length < 3}
          style={{ padding: "8px 16px", cursor: submitting ? "default" : "pointer", borderRadius: 6 }}
        >
          {submitting ? "Submitting…" : "Submit"}
        </button>
      </section>

      {err && <p style={{ color: "var(--danger)", fontSize: 14 }}>{err}</p>}
      {!requests && !err && <p style={{ color: "var(--muted-strong)" }}>Loading…</p>}
      {requests && requests.length === 0 && <p style={{ color: "var(--muted-strong)" }}>Nothing suggested yet — be the first.</p>}

      {requests?.map((r) => (
        <div key={r.id} style={{ display: "flex", gap: 12, border: "1px solid var(--border)", borderRadius: 8, padding: 12, marginBottom: 8, background: "var(--surface)" }}>
          <button
            onClick={() => vote(r)}
            disabled={voting === r.id}
            aria-pressed={r.voted_by_me}
            title={r.voted_by_me ? "Remove your upvote" : "Upvote"}
            style={{
              display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center",
              width: 44, minWidth: 44, borderRadius: 6, cursor: "pointer", fontSize: 13, padding: "6px 0",
              border: `1px solid ${r.voted_by_me ? "var(--accent)" : "var(--border)"}`,
              background: r.voted_by_me ? "var(--accent)" : "transparent",
              color: r.voted_by_me ? "var(--on-accent, #fff)" : "inherit",
            }}
          >
            <span style={{ fontSize: 14, lineHeight: 1 }}>▲</span>
            <span style={{ fontWeight: 600 }}>{r.votes}</span>
          </button>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ display: "flex", alignItems: "baseline", gap: 8, flexWrap: "wrap" }}>
              <strong style={{ fontSize: 14 }}>{r.title}</strong>
              <span style={{ fontSize: 11, color: STATUS_COLOR[r.status] }}>{STATUS_LABEL[r.status]}</span>
            </div>
            {r.description && <p style={{ fontSize: 13, color: "var(--muted-strong)", margin: "4px 0 0" }}>{r.description}</p>}
            <p style={{ fontSize: 12, color: "var(--muted)", margin: "6px 0 0" }}>
              {r.submitted_by} · {new Date(r.created_at).toLocaleDateString()}
            </p>
            {isAdmin && (
              <div style={{ marginTop: 8 }}>
                <select
                  value={r.status}
                  disabled={statusBusy === r.id}
                  onChange={(e) => setStatus(r, e.target.value as FeatureRequest["status"])}
                  style={{ fontSize: 12, padding: "3px 6px" }}
                >
                  {(Object.keys(STATUS_LABEL) as FeatureRequest["status"][]).map((s) => (
                    <option key={s} value={s}>{STATUS_LABEL[s]}</option>
                  ))}
                </select>
                {r.status === "planned" && (
                  <span style={{ fontSize: 11, color: "var(--muted)", marginLeft: 8 }}>
                    Queued for the next auto-build run
                  </span>
                )}
              </div>
            )}
          </div>
        </div>
      ))}
    </main>
  );
}

export default function RequestsPage() {
  return <RequireMember next="/requests">{() => (<><Nav /><RequestsView /></>)}</RequireMember>;
}
