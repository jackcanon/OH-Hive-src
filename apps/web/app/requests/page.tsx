"use client";

import { useEffect, useState, type CSSProperties } from "react";
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

// Bug reports (feature request b8ae4a1e-8151-4017-88cb-c23505a7b2d8, Jack, 2026-09-12): "a space to
// provide feedback on features that should be working but don't... upload logs and screenshots...
// signed or anonymous... comment section... follow to be alerted when resolved." Real notification
// delivery is out of v1 scope (no push/email infra exists yet) -- following just records the flag,
// documented in the migration as a fast-follow once that infra exists.

type BugReport = {
  id: string;
  title: string;
  description: string;
  status: "open" | "investigating" | "resolved" | "wont_fix";
  created_at: string;
  resolved_at: string | null;
  anonymous: boolean;
  submitted_by: string | null; // null when anonymous, even to admins
  is_mine: boolean;
  attachments: string[];
  comment_count: number;
  following: boolean;
};

type BugComment = { id: string; body: string; created_at: string; author: string };

const BUG_STATUS_LABEL: Record<BugReport["status"], string> = {
  open: "Open",
  investigating: "Investigating",
  resolved: "Resolved",
  wont_fix: "Won't fix",
};
const BUG_STATUS_COLOR: Record<BugReport["status"], string> = {
  open: "var(--muted-strong)",
  investigating: "var(--accent)",
  resolved: "var(--ok)",
  wont_fix: "var(--muted)",
};

const ALLOWED_ATTACHMENT_TYPES = ["image/png", "image/jpeg", "image/webp", "image/gif", "text/plain", "application/json", "application/zip", "application/gzip"];
const MAX_ATTACHMENT_BYTES = 10 * 1024 * 1024;
const IMAGE_EXT = /\.(png|jpe?g|webp|gif)$/i;

const inputStyle: CSSProperties = { width: "100%", padding: "8px 10px", marginBottom: 8, fontSize: 14, borderRadius: 6, border: "1px solid var(--border)", background: "var(--bg)", color: "inherit" };

function FeatureRequestsView() {
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
    <>
      <section style={{ border: "1px solid var(--border)", borderRadius: 10, padding: 16, marginBottom: 28, background: "var(--surface)" }}>
        <h2 style={{ fontSize: 15, margin: "0 0 10px" }}>Suggest something</h2>
        <input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="One line: what should Hive do?"
          maxLength={120}
          style={inputStyle}
        />
        <textarea
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="Anything that'd help us understand it — optional."
          maxLength={4000}
          rows={3}
          style={{ ...inputStyle, resize: "vertical" }}
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
    </>
  );
}

function BugReportsView() {
  const [reports, setReports] = useState<BugReport[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [anonymous, setAnonymous] = useState(false);
  const [files, setFiles] = useState<File[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [isAdmin, setIsAdmin] = useState(false);
  const [statusBusy, setStatusBusy] = useState<string | null>(null);
  const [followBusy, setFollowBusy] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [comments, setComments] = useState<Record<string, BugComment[]>>({});
  const [commentDraft, setCommentDraft] = useState("");
  const [commentBusy, setCommentBusy] = useState(false);

  const load = () =>
    supabaseBrowser().rpc("hive_bug_report_list", {}).then(({ data, error }) => {
      if (error) setErr(friendlyError(error.message));
      else setReports((data ?? []) as BugReport[]);
    });

  useEffect(() => {
    load();
    supabaseBrowser().rpc("hive_am_i_admin", {}).then(({ data }) => setIsAdmin(!!data));
  }, []);

  function onFilesSelected(list: FileList | null) {
    if (!list) return;
    setErr(null);
    const picked: File[] = [];
    for (const f of Array.from(list)) {
      if (!ALLOWED_ATTACHMENT_TYPES.includes(f.type)) { setErr(`"${f.name}" isn't a supported type — screenshots (PNG/JPEG/WEBP/GIF) or logs (TXT/JSON/ZIP/GZ).`); continue; }
      if (f.size > MAX_ATTACHMENT_BYTES) { setErr(`"${f.name}" is too large — 10MB max.`); continue; }
      picked.push(f);
    }
    setFiles((prev) => [...prev, ...picked]);
  }

  const submit = async () => {
    if (title.trim().length < 3) { setErr("Give it a few more words — at least 3 characters."); return; }
    setSubmitting(true);
    setErr(null);
    const { data: created, error } = await supabaseBrowser().rpc("hive_bug_report_create", {
      p_title: title.trim(),
      p_description: description.trim(),
      p_anonymous: anonymous,
    });
    if (error) { setErr(friendlyError(error.message)); setSubmitting(false); return; }
    const reportId = (created as { id: string }).id;

    if (files.length > 0) {
      const sb = supabaseBrowser();
      const { data: userData } = await sb.auth.getUser();
      const uid = userData.user?.id;
      for (const file of files) {
        if (!uid) break;
        const path = `${uid}/${reportId}-${crypto.randomUUID()}-${file.name}`;
        const { error: uploadError } = await sb.storage.from("bug-attachments").upload(path, file, { contentType: file.type });
        if (uploadError) { setErr(`Report saved, but "${file.name}" failed to upload — try attaching it again isn't supported yet, sorry.`); continue; }
        const { data: pub } = sb.storage.from("bug-attachments").getPublicUrl(path);
        await sb.rpc("hive_bug_report_add_attachment", { p_bug_report_id: reportId, p_url: pub.publicUrl });
      }
    }

    setSubmitting(false);
    setTitle("");
    setDescription("");
    setAnonymous(false);
    setFiles([]);
    load();
  };

  async function setStatus(r: BugReport, status: BugReport["status"]) {
    setStatusBusy(r.id);
    const { error } = await supabaseBrowser().rpc("hive_admin_bug_report_set_status", { p_bug_report_id: r.id, p_status: status });
    setStatusBusy(null);
    if (error) { setErr(friendlyError(error.message)); return; }
    load();
  }

  async function toggleFollow(r: BugReport) {
    setFollowBusy(r.id);
    const { error } = await supabaseBrowser().rpc("hive_bug_report_follow", { p_bug_report_id: r.id, p_on: !r.following });
    setFollowBusy(null);
    if (error) { setErr(friendlyError(error.message)); return; }
    load();
  }

  async function toggleExpand(r: BugReport) {
    if (expanded === r.id) { setExpanded(null); return; }
    setExpanded(r.id);
    setCommentDraft("");
    if (!comments[r.id]) {
      const { data, error } = await supabaseBrowser().rpc("hive_bug_report_comment_list", { p_bug_report_id: r.id });
      if (!error) setComments((prev) => ({ ...prev, [r.id]: (data ?? []) as BugComment[] }));
    }
  }

  async function addComment(r: BugReport) {
    if (!commentDraft.trim()) return;
    setCommentBusy(true);
    const { error } = await supabaseBrowser().rpc("hive_bug_report_comment_add", { p_bug_report_id: r.id, p_body: commentDraft.trim() });
    setCommentBusy(false);
    if (error) { setErr(friendlyError(error.message)); return; }
    setCommentDraft("");
    const { data } = await supabaseBrowser().rpc("hive_bug_report_comment_list", { p_bug_report_id: r.id });
    setComments((prev) => ({ ...prev, [r.id]: (data ?? []) as BugComment[] }));
    load(); // refresh comment_count on the collapsed row
  }

  return (
    <>
      <section style={{ border: "1px solid var(--border)", borderRadius: 10, padding: 16, marginBottom: 28, background: "var(--surface)" }}>
        <h2 style={{ fontSize: 15, margin: "0 0 10px" }}>Report a bug</h2>
        <input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="One line: what's broken?"
          maxLength={120}
          style={inputStyle}
        />
        <textarea
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="What did you expect, what happened instead, and how to reproduce it — optional."
          maxLength={4000}
          rows={3}
          style={{ ...inputStyle, resize: "vertical" }}
        />
        <div style={{ marginBottom: 10 }}>
          <label style={{ fontSize: 12, color: "var(--muted-strong)", cursor: "pointer" }}>
            <input type="checkbox" checked={anonymous} onChange={(e) => setAnonymous(e.target.checked)} style={{ marginRight: 6, verticalAlign: "middle" }} />
            Submit anonymously — your name won't be shown to anyone, including admins
          </label>
        </div>
        <div style={{ marginBottom: 10 }}>
          <input type="file" multiple accept={ALLOWED_ATTACHMENT_TYPES.join(",")} onChange={(e) => onFilesSelected(e.target.files)} style={{ fontSize: 12 }} />
          {files.length > 0 && (
            <ul style={{ margin: "6px 0 0", padding: 0, listStyle: "none", fontSize: 12, color: "var(--muted-strong)" }}>
              {files.map((f, i) => (
                <li key={i} style={{ display: "flex", justifyContent: "space-between", gap: 8 }}>
                  <span>{f.name} ({Math.round(f.size / 1024)}KB)</span>
                  <button onClick={() => setFiles((prev) => prev.filter((_, j) => j !== i))} style={{ fontSize: 11, padding: "1px 6px" }}>Remove</button>
                </li>
              ))}
            </ul>
          )}
        </div>
        <button
          onClick={submit}
          disabled={submitting || title.trim().length < 3}
          style={{ padding: "8px 16px", cursor: submitting ? "default" : "pointer", borderRadius: 6 }}
        >
          {submitting ? "Submitting…" : "Submit"}
        </button>
      </section>

      {err && <p style={{ color: "var(--danger)", fontSize: 14 }}>{err}</p>}
      {!reports && !err && <p style={{ color: "var(--muted-strong)" }}>Loading…</p>}
      {reports && reports.length === 0 && <p style={{ color: "var(--muted-strong)" }}>No bug reports yet.</p>}

      {reports?.map((r) => (
        <div key={r.id} style={{ border: "1px solid var(--border)", borderRadius: 8, padding: 12, marginBottom: 8, background: "var(--surface)" }}>
          <div style={{ display: "flex", alignItems: "baseline", gap: 8, flexWrap: "wrap" }}>
            <strong style={{ fontSize: 14 }}>{r.title}</strong>
            <span style={{ fontSize: 11, color: BUG_STATUS_COLOR[r.status] }}>{BUG_STATUS_LABEL[r.status]}</span>
            {r.anonymous && <span style={{ fontSize: 11, color: "var(--muted)" }}>· Anonymous</span>}
          </div>
          {r.description && <p style={{ fontSize: 13, color: "var(--muted-strong)", margin: "4px 0 0" }}>{r.description}</p>}

          {r.attachments.length > 0 && (
            <div style={{ display: "flex", gap: 8, flexWrap: "wrap", margin: "8px 0 0" }}>
              {r.attachments.map((url) =>
                IMAGE_EXT.test(url) ? (
                  <a key={url} href={url} target="_blank" rel="noreferrer">
                    <img src={url} alt="attachment" style={{ width: 64, height: 64, objectFit: "cover", borderRadius: 6, border: "1px solid var(--border)" }} />
                  </a>
                ) : (
                  <a key={url} href={url} target="_blank" rel="noreferrer" style={{ fontSize: 12, border: "1px solid var(--border)", borderRadius: 6, padding: "4px 8px" }}>
                    {decodeURIComponent(url.split("/").pop() ?? "file")}
                  </a>
                )
              )}
            </div>
          )}

          <p style={{ fontSize: 12, color: "var(--muted)", margin: "8px 0 0" }}>
            {r.submitted_by ?? "Anonymous"} · {new Date(r.created_at).toLocaleDateString()}
          </p>

          <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 8, flexWrap: "wrap" }}>
            <button onClick={() => toggleExpand(r)} style={{ fontSize: 12, padding: "4px 10px" }}>
              {expanded === r.id ? "Hide comments" : `Comments (${r.comment_count})`}
            </button>
            <button
              onClick={() => toggleFollow(r)}
              disabled={followBusy === r.id}
              aria-pressed={r.following}
              style={{
                fontSize: 12, padding: "4px 10px",
                border: `1px solid ${r.following ? "var(--accent)" : "var(--border)"}`,
                background: r.following ? "var(--accent)" : "transparent",
                color: r.following ? "var(--on-accent, #fff)" : "inherit",
              }}
            >
              {r.following ? "Following" : "Follow for updates"}
            </button>
            {isAdmin && (
              <>
                <select
                  value={r.status}
                  disabled={statusBusy === r.id}
                  onChange={(e) => setStatus(r, e.target.value as BugReport["status"])}
                  style={{ fontSize: 12, padding: "3px 6px" }}
                >
                  {(Object.keys(BUG_STATUS_LABEL) as BugReport["status"][]).map((s) => (
                    <option key={s} value={s}>{BUG_STATUS_LABEL[s]}</option>
                  ))}
                </select>
                {r.status === "resolved" && r.resolved_at && (
                  <span style={{ fontSize: 11, color: "var(--muted)" }}>
                    Resolved {new Date(r.resolved_at).toLocaleDateString()}
                  </span>
                )}
              </>
            )}
          </div>

          {expanded === r.id && (
            <div style={{ marginTop: 10, paddingTop: 10, borderTop: "1px solid var(--border)" }}>
              {(comments[r.id] ?? []).length === 0 && <p style={{ fontSize: 12, color: "var(--muted)", margin: 0 }}>No comments yet.</p>}
              {(comments[r.id] ?? []).map((c) => (
                <div key={c.id} style={{ marginBottom: 8 }}>
                  <div style={{ fontSize: 12, color: "var(--muted-strong)" }}>
                    <strong>{c.author}</strong> · {new Date(c.created_at).toLocaleDateString()}
                  </div>
                  <p style={{ fontSize: 13, margin: "2px 0 0" }}>{c.body}</p>
                </div>
              ))}
              <div style={{ display: "flex", gap: 6, marginTop: 8 }}>
                <input
                  value={commentDraft}
                  onChange={(e) => setCommentDraft(e.target.value)}
                  placeholder="Add a comment…"
                  maxLength={4000}
                  style={{ flex: 1, padding: "6px 8px", fontSize: 13, borderRadius: 6, border: "1px solid var(--border)", background: "var(--bg)", color: "inherit" }}
                  onKeyDown={(e) => { if (e.key === "Enter") addComment(r); }}
                />
                <button onClick={() => addComment(r)} disabled={commentBusy || !commentDraft.trim()} style={{ fontSize: 13, padding: "6px 12px" }}>
                  Post
                </button>
              </div>
            </div>
          )}
        </div>
      ))}
    </>
  );
}

function RequestsView() {
  const [tab, setTab] = useState<"features" | "bugs">("features");

  const tabStyle = (active: boolean): CSSProperties => ({
    padding: "8px 14px", fontSize: 13, borderRadius: 6, cursor: "pointer",
    border: `1px solid ${active ? "var(--accent)" : "var(--border)"}`,
    background: active ? "var(--accent)" : "transparent",
    color: active ? "var(--on-accent, #fff)" : "inherit",
  });

  return (
    <main style={{ maxWidth: 720, margin: "0 auto", padding: 24 }}>
      <h1 style={{ margin: "8px 0" }}>{tab === "features" ? "Feature requests" : "Bug reports"}</h1>
      <p style={{ color: "var(--muted-strong)", fontSize: 14, margin: "0 0 16px" }}>
        {tab === "features"
          ? "Tell us what you want Hive to do next. Everyone can see and upvote what's been suggested."
          : "Something not working as it should? Report it here — signed or anonymous, with screenshots or logs if it helps."}
      </p>

      <div style={{ display: "flex", gap: 8, marginBottom: 20 }}>
        <button onClick={() => setTab("features")} style={tabStyle(tab === "features")}>Feature requests</button>
        <button onClick={() => setTab("bugs")} style={tabStyle(tab === "bugs")}>Bug reports</button>
      </div>

      {tab === "features" ? <FeatureRequestsView /> : <BugReportsView />}
    </main>
  );
}

export default function RequestsPage() {
  return <RequireMember next="/requests">{() => (<><Nav /><RequestsView /></>)}</RequireMember>;
}
