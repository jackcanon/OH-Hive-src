"use client";

import { useEffect, useState } from "react";
import { AboutSection } from "@ohhive/ui";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav, RequireMember } from "@/components/RequireMember";

type Me = {
  member: { status: string; onramp: string | null; since: string; invited_by: string | null } | null;
  profile: { display_name: string; email: string } | null;
  invites: { code: string; uses: number; max_uses: number; note: string; expires_at: string; revoked: boolean }[];
};

function SettingsView() {
  const [me, setMe] = useState<Me | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);

  const load = () => supabaseBrowser().rpc("hive_me").then(({ data, error }) => { if (error) setErr(error.message); else setMe(data as Me); });
  useEffect(() => { load(); }, []);

  async function mint() {
    setBusy(true);
    const { error } = await supabaseBrowser().rpc("hive_invite_create", { p_max_uses: 5, p_days: 30, p_note: note });
    setBusy(false);
    if (error) setErr(error.message); else { setNote(""); load(); }
  }
  async function revoke(code: string) {
    const { error } = await supabaseBrowser().rpc("hive_invite_revoke", { p_code: code });
    if (error) setErr(error.message); else load();
  }

  const origin = typeof location !== "undefined" ? location.origin : "https://ohghive.com";

  return (
    <main style={{ maxWidth: 720, margin: "0 auto", padding: 24 }}>
      <h1 style={{ margin: "8px 0" }}>Settings</h1>
      {err && <p style={{ color: "#b00020" }}>{err}</p>}
      {me?.profile && (
        <p style={{ color: "#555" }}>
          {me.profile.display_name} · {me.profile.email}
          {me.member && <> · member since {new Date(me.member.since).toLocaleDateString()}{me.member.invited_by && `, invited by ${me.member.invited_by}`}</>}
        </p>
      )}

      <h2 style={{ fontSize: 16, marginTop: 28 }}>Invite people</h2>
      <p style={{ color: "#666", fontSize: 13 }}>Each code works 5 times for 30 days. The Hive is invite-only — hand these to people you'd vouch for.</p>
      <div style={{ display: "flex", gap: 8 }}>
        <input value={note} onChange={(e) => setNote(e.target.value)} placeholder="note to self, e.g. “OH Global Tuesday crew”" style={{ flex: 1, padding: 8 }} />
        <button onClick={mint} disabled={busy} style={{ padding: "8px 14px", cursor: "pointer" }}>New invite</button>
      </div>
      {me?.invites.map((i) => (
        <div key={i.code} style={{ border: "1px solid #e6e2d6", borderRadius: 8, padding: 10, marginTop: 8, background: "#fff", fontSize: 13,
             opacity: i.revoked ? 0.5 : 1 }}>
          <code style={{ fontSize: 14 }}>{origin}/join?code={i.code}</code>
          <div style={{ color: "#777", marginTop: 4 }}>
            {i.uses}/{i.max_uses} used · expires {new Date(i.expires_at).toLocaleDateString()}{i.note && ` · ${i.note}`}{i.revoked && " · revoked"}
            {!i.revoked && <> · <a href="#" onClick={(e) => { e.preventDefault(); revoke(i.code); }}>revoke</a></>}
          </div>
        </div>
      ))}

      <h2 style={{ fontSize: 16, marginTop: 32 }}>About</h2>
      <AboutSection info={{ app_version: "0.1.0", core_version: "web", made_by: "Happy Jack Media", made_by_url: "https://happyjack.media",
                            blog_name: "This Is Not A Draft", blog_url: "https://thisisnotadraft.com" }} />
    </main>
  );
}

export default function Settings() {
  return <RequireMember next="/settings">{() => (<><Nav /><SettingsView /></>)}</RequireMember>;
}
