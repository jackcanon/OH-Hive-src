"use client";

import { useEffect, useState, type ReactNode } from "react";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav, RequireMember } from "@/components/RequireMember";
import { Avatar } from "@/components/Avatar";
import { friendlyError } from "@/lib/errors";

// Everyone's directory (Jack, 2026-09-12): "we can make it so everyone can see the member list" --
// unlike /admin's members table (name, email, wallet -- admin-only), this carries no PII: just
// avatar, bio, and live presence from hive.member_directory().

type Member = {
  id: string;
  display_name: string;
  avatar_choice: string;
  google_avatar_url: string | null;
  bio: string;
  is_admin: boolean;
  joined: string;
  online: boolean;
  working: boolean;
  hosting: boolean;
  regions: string[];
};

function Badge({ children, tone }: { children: ReactNode; tone: "ok" | "accent" | "muted" }) {
  const color = tone === "ok" ? "var(--ok)" : tone === "accent" ? "var(--accent)" : "var(--muted)";
  return (
    <span style={{ fontSize: 11, padding: "2px 7px", borderRadius: 10, border: `1px solid ${color}`, color, whiteSpace: "nowrap" }}>
      {children}
    </span>
  );
}

function MembersView() {
  const [members, setMembers] = useState<Member[] | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    supabaseBrowser().rpc("hive_member_directory", {}).then(({ data, error }) => {
      if (error) setErr(friendlyError(error.message));
      else setMembers((data ?? []) as Member[]);
    });
  }, []);

  return (
    <main style={{ maxWidth: 780, margin: "0 auto", padding: 24 }}>
      <h1 style={{ margin: "8px 0" }}>Members</h1>
      <p style={{ color: "var(--muted-strong)", fontSize: 14, margin: "0 0 24px" }}>Who's in the Hive right now.</p>

      {err && <p style={{ color: "var(--danger)" }}>{err}</p>}
      {!members && !err && <p style={{ color: "var(--muted-strong)" }}>Loading…</p>}

      {members?.map((m) => (
        <div key={m.id} style={{ display: "flex", gap: 12, border: "1px solid var(--border)", borderRadius: 8, padding: 12, marginBottom: 8, background: "var(--surface)" }}>
          <Avatar choice={m.avatar_choice} googleUrl={m.google_avatar_url} name={m.display_name} size={44} />
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
              <strong style={{ fontSize: 14 }}>{m.display_name}</strong>
              {m.is_admin && <Badge tone="accent">Admin</Badge>}
              {m.online && <Badge tone="ok">Online</Badge>}
              {m.working && <Badge tone="accent">Working</Badge>}
              {m.hosting && <Badge tone="muted">Hosting{m.regions.length > 0 ? ` · ${m.regions.join(", ")}` : ""}</Badge>}
            </div>
            {m.bio && <p style={{ fontSize: 13, color: "var(--muted-strong)", margin: "6px 0 0" }}>{m.bio}</p>}
            <p style={{ fontSize: 11, color: "var(--muted)", margin: "6px 0 0" }}>
              Member since {new Date(m.joined).toLocaleDateString()}
            </p>
          </div>
        </div>
      ))}
    </main>
  );
}

export default function MembersPage() {
  return <RequireMember next="/members">{() => (<><Nav /><MembersView /></>)}</RequireMember>;
}
