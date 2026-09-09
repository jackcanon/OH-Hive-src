"use client";

import { useEffect, useState } from "react";
import { AboutSection } from "@ohhive/ui";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav, RequireMember } from "@/components/RequireMember";
import { friendlyError } from "@/lib/errors";

type Keys = Record<string, { last4: string; since: string }>;
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
  const [keys, setKeys] = useState<Keys>({});
  const [keyProvider, setKeyProvider] = useState<"anthropic" | "openai" | "nous">("anthropic");
  const [keyValue, setKeyValue] = useState("");
  const [linkCode, setLinkCode] = useState<string | null>(null);
  const [linkBusy, setLinkBusy] = useState(false);

  const load = () => {
    supabaseBrowser().rpc("hive_me").then(({ data, error }) => { if (error) setErr(friendlyError(error.message)); else setMe(data as Me); });
    supabaseBrowser().rpc("hive_member_keys_status").then(({ data }) => { if (data) setKeys(data as Keys); });
  };
  useEffect(() => { load(); }, []);

  async function saveKey() {
    if (!keyValue.trim()) return;
    setBusy(true);
    const { error } = await supabaseBrowser().rpc("hive_member_key_set", { p_provider: keyProvider, p_key: keyValue });
    setBusy(false);
    if (error) setErr(friendlyError(error.message)); else { setKeyValue(""); load(); }
  }
  async function removeKey(provider: string) {
    const { error } = await supabaseBrowser().rpc("hive_member_key_remove", { p_provider: provider });
    if (error) setErr(friendlyError(error.message)); else load();
  }

  async function mint() {
    setBusy(true);
    const { error } = await supabaseBrowser().rpc("hive_invite_create", { p_max_uses: 5, p_days: 30, p_note: note });
    setBusy(false);
    if (error) setErr(friendlyError(error.message)); else { setNote(""); load(); }
  }
  async function revoke(code: string) {
    const { error } = await supabaseBrowser().rpc("hive_invite_revoke", { p_code: code });
    if (error) setErr(friendlyError(error.message)); else load();
  }

  // ADR-020: 15-minute single-use code, redeemed by DMing the Telegram bot "/link <code>" -- see
  // docs/TELEGRAM-INTEGRATION-PLAN.md for the bot setup this depends on.
  async function generateLinkCode() {
    setLinkBusy(true);
    const { data, error } = await supabaseBrowser().rpc("hive_member_create_link_code");
    setLinkBusy(false);
    if (error) setErr(friendlyError(error.message)); else setLinkCode(data as string);
  }

  const origin = typeof location !== "undefined" ? location.origin : "https://ohghive.com";

  const KEY_INFO: Record<"anthropic" | "openai" | "nous", { label: string; href: string; placeholder: string; note?: string }> = {
    anthropic: { label: "Anthropic", href: "https://console.anthropic.com/settings/keys", placeholder: "sk-ant-…" },
    openai: { label: "OpenAI", href: "https://platform.openai.com/api-keys", placeholder: "sk-…" },
    nous: {
      label: "Nous (Hermes)", href: "https://portal.nousresearch.com/manage-subscription", placeholder: "your Nous Portal key",
      note: "Used as a fallback for the interviewer if your Anthropic and OpenAI keys aren't set or fail.",
    },
  };

  return (
    <main style={{ maxWidth: 720, margin: "0 auto", padding: 24 }}>
      <h1 style={{ margin: "8px 0" }}>Settings</h1>
      {err && <p style={{ color: "var(--danger)" }}>{err}</p>}
      {me?.profile && (
        <p style={{ color: "var(--muted-strong)" }}>
          {me.profile.display_name} · {me.profile.email}
          {me.member && <> · member since {new Date(me.member.since).toLocaleDateString()}{me.member.invited_by && `, invited by ${me.member.invited_by}`}</>}
        </p>
      )}

      <h2 style={{ fontSize: 16, marginTop: 28 }}>Invite people</h2>
      <p style={{ color: "var(--muted-strong)", fontSize: 13 }}>Each code works 5 times for 30 days. The Hive is invite-only — hand these to people you'd vouch for.</p>
      <div style={{ display: "flex", gap: 8 }}>
        <input value={note} onChange={(e) => setNote(e.target.value)} placeholder="note to self, e.g. “OH Global Tuesday crew”" style={{ flex: 1, padding: 8 }} />
        <button onClick={mint} disabled={busy} style={{ padding: "8px 14px", cursor: "pointer" }}>New invite</button>
      </div>
      {me?.invites.map((i) => (
        <div key={i.code} style={{ border: "1px solid var(--border)", borderRadius: 8, padding: 10, marginTop: 8, background: "var(--surface)", fontSize: 13,
             opacity: i.revoked ? 0.5 : 1 }}>
          <code style={{ fontSize: 14 }}>{origin}/join?code={i.code}</code>
          <div style={{ color: "var(--muted)", marginTop: 4 }}>
            {i.uses}/{i.max_uses} used · expires {new Date(i.expires_at).toLocaleDateString()}{i.note && ` · ${i.note}`}{i.revoked && " · revoked"}
            {!i.revoked && <> · <a href="#" onClick={(e) => { e.preventDefault(); revoke(i.code); }}>revoke</a></>}
          </div>
        </div>
      ))}

      <h2 id="keys" style={{ fontSize: 16, marginTop: 32 }}>Your own AI key (optional)</h2>
      <p style={{ color: "var(--muted-strong)", fontSize: 13 }}>
        The project interviewer runs on a frontier model. With your own key it runs on your account and costs the Hive nothing;
        without one, the hub's key is used and charged to your purchased Honey. Keys are stored encrypted (Supabase Vault) and only ever read by the interviewer.
        Don't have one yet? {(["anthropic", "openai", "nous"] as const).map((p, i) => (
          <span key={p}>
            {i > 0 && " · "}
            <a href={KEY_INFO[p].href} target="_blank" rel="noreferrer">Get an {KEY_INFO[p].label} key ↗</a>
          </span>
        ))}
      </p>
      {Object.entries(keys).map(([prov, k]) => (
        <div key={prov} style={{ border: "1px solid var(--border)", borderRadius: 8, padding: 10, marginBottom: 8, background: "var(--surface)", fontSize: 13, display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <span><strong>{KEY_INFO[prov as keyof typeof KEY_INFO]?.label ?? prov}</strong> · ····{k.last4} · added {new Date(k.since).toLocaleDateString()}</span>
          <a href="#" onClick={(e) => { e.preventDefault(); removeKey(prov); }}>remove</a>
        </div>
      ))}
      <div style={{ display: "flex", gap: 8 }}>
        <select value={keyProvider} onChange={(e) => setKeyProvider(e.target.value as "anthropic" | "openai" | "nous")} style={{ padding: 8 }}>
          <option value="anthropic">Anthropic</option>
          <option value="openai">OpenAI</option>
          <option value="nous">Nous (Hermes)</option>
        </select>
        <input type="password" value={keyValue} onChange={(e) => setKeyValue(e.target.value)} placeholder={KEY_INFO[keyProvider].placeholder} style={{ flex: 1, padding: 8 }} autoComplete="off" />
        <button onClick={saveKey} disabled={busy || keyValue.trim().length < 20} style={{ padding: "8px 14px", cursor: "pointer" }}>Save key</button>
      </div>
      {KEY_INFO[keyProvider].note && <p style={{ color: "var(--muted)", fontSize: 12, marginTop: 4 }}>{KEY_INFO[keyProvider].note}</p>}

      <h2 style={{ fontSize: 16, marginTop: 32 }}>Chat notifications (Telegram)</h2>
      <p style={{ color: "var(--muted-strong)", fontSize: 13 }}>
        Get job completion/failure updates and chat with the Hive assistant from Telegram. Generate a code below, then
        message the bot <code>/link &lt;code&gt;</code> to connect your account. Codes expire in 15 minutes and work once.
      </p>
      <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <button onClick={generateLinkCode} disabled={linkBusy} style={{ padding: "8px 14px", cursor: "pointer" }}>Generate link code</button>
        {linkCode && (
          <code style={{ fontSize: 15, padding: "6px 10px", background: "var(--surface)", border: "1px solid var(--border)", borderRadius: 6 }}>
            /link {linkCode}
          </code>
        )}
      </div>

      <h2 style={{ fontSize: 16, marginTop: 32 }}>About</h2>
      <AboutSection info={{ app_version: "0.3.0", core_version: "web", made_by: "Happy Jack Media", made_by_url: "https://happyjack.media",
                            blog_name: "This Is Not A Draft", blog_url: "https://thisisnotadraft.com" }} />
    </main>
  );
}

export default function Settings() {
  return <RequireMember next="/settings">{() => (<><Nav /><SettingsView /></>)}</RequireMember>;
}
