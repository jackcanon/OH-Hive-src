"use client";

import { useEffect, useState } from "react";
import { AboutSection } from "@hive/ui";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav, RequireMember } from "@/components/RequireMember";
import { friendlyError } from "@/lib/errors";
import { Avatar, PRESETS } from "@/components/Avatar";

type Keys = Record<string, { last4: string; since: string }>;
type Me = {
  member: { status: string; onramp: string | null; since: string; invited_by: string | null; bio: string; avatar_choice: string; custom_avatar_url: string | null } | null;
  profile: { display_name: string; email: string; google_avatar_url: string | null } | null;
  invites: { code: string; uses: number; max_uses: number; note: string; expires_at: string; revoked: boolean }[];
};

const MAX_AVATAR_BYTES = 2 * 1024 * 1024;
const ALLOWED_AVATAR_TYPES = ["image/png", "image/jpeg", "image/webp", "image/gif"];

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
  const [linkCopied, setLinkCopied] = useState(false);
  const [bio, setBio] = useState("");
  const [avatarChoice, setAvatarChoice] = useState("google");
  const [customAvatarUrl, setCustomAvatarUrl] = useState<string | null>(null);
  const [profileBusy, setProfileBusy] = useState(false);
  const [profileSaved, setProfileSaved] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [uploadErr, setUploadErr] = useState<string | null>(null);

  const load = () => {
    supabaseBrowser().rpc("hive_me").then(({ data, error }) => {
      if (error) { setErr(friendlyError(error.message)); return; }
      const m = data as Me;
      setMe(m);
      if (m.member) { setBio(m.member.bio); setAvatarChoice(m.member.avatar_choice); setCustomAvatarUrl(m.member.custom_avatar_url); }
    });
    supabaseBrowser().rpc("hive_member_keys_status").then(({ data }) => { if (data) setKeys(data as Keys); });
  };
  useEffect(() => { load(); }, []);

  async function saveProfile(overrideAvatarChoice?: string, overrideCustomUrl?: string) {
    setProfileBusy(true);
    setProfileSaved(false);
    const { error } = await supabaseBrowser().rpc("hive_member_update_profile", {
      p_bio: bio.trim(),
      p_avatar_choice: overrideAvatarChoice ?? avatarChoice,
      p_custom_avatar_url: overrideCustomUrl ?? null,
    });
    setProfileBusy(false);
    if (error) { setErr(friendlyError(error.message)); return; }
    setProfileSaved(true);
    load();
    setTimeout(() => setProfileSaved(false), 2000);
  }

  // Jack, 2026-09-12: his Google Workspace account doesn't hand back a profile photo over OAuth
  // at all, so presets alone aren't enough -- upload your own. One fixed path per member
  // (`<uid>/avatar`, upsert) so re-uploading never leaves an orphaned old file behind; the `?v=`
  // cache-buster is what makes a re-upload actually show up instead of the CDN's cached old image.
  async function onAvatarFileSelected(file: File) {
    setUploadErr(null);
    if (!ALLOWED_AVATAR_TYPES.includes(file.type)) { setUploadErr("Use a PNG, JPEG, WEBP, or GIF."); return; }
    if (file.size > MAX_AVATAR_BYTES) { setUploadErr("That image is too large — 2MB max."); return; }
    setUploading(true);
    const sb = supabaseBrowser();
    const { data: userData } = await sb.auth.getUser();
    const uid = userData.user?.id;
    if (!uid) { setUploadErr("Couldn't confirm your session — try reloading."); setUploading(false); return; }
    const path = `${uid}/avatar`;
    const { error: uploadError } = await sb.storage.from("avatars").upload(path, file, { upsert: true, contentType: file.type });
    if (uploadError) { setUploadErr("Upload failed — try again."); setUploading(false); return; }
    const { data: pub } = sb.storage.from("avatars").getPublicUrl(path);
    const versioned = `${pub.publicUrl}?v=${Date.now()}`;
    setUploading(false);
    setAvatarChoice("custom");
    setCustomAvatarUrl(versioned);
    await saveProfile("custom", versioned);
  }

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
    if (error) setErr(friendlyError(error.message)); else { setLinkCode(data as string); setLinkCopied(false); }
  }

  async function copyLinkCommand() {
    if (!linkCode) return;
    try {
      await navigator.clipboard.writeText(`/link ${linkCode}`);
      setLinkCopied(true);
      setTimeout(() => setLinkCopied(false), 2000);
    } catch { /* clipboard permission denied -- the text is still visible to copy by hand */ }
  }

  const origin = typeof location !== "undefined" ? location.origin : "https://ohghive.com";

  const KEY_INFO: Record<"anthropic" | "openai" | "nous", { label: string; href: string; placeholder: string; note?: string }> = {
    anthropic: { label: "Anthropic", href: "https://console.anthropic.com/settings/keys", placeholder: "sk-ant-…" },
    openai: { label: "OpenAI", href: "https://platform.openai.com/api-keys", placeholder: "sk-…" },
    nous: {
      label: "Nous (Hermes)", href: "https://portal.nousresearch.com/manage-subscription", placeholder: "your Nous Portal key",
      note: "Used as a fallback for the project chat if your Anthropic and OpenAI keys aren't set or fail.",
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

      <h2 style={{ fontSize: 16, marginTop: 28 }}>Your profile</h2>
      <p style={{ color: "var(--muted-strong)", fontSize: 13 }}>
        Shown on the <a href="/members">Members</a> page — a picture, and a line about you. Nobody sees your email there.
      </p>
      <div style={{ display: "flex", gap: 14, alignItems: "flex-start", marginBottom: 4 }}>
        <Avatar choice={avatarChoice} googleUrl={me?.profile?.google_avatar_url} customUrl={customAvatarUrl} name={me?.profile?.display_name ?? ""} size={56} />
        <div style={{ display: "flex", flexWrap: "wrap", gap: 8, flex: 1 }}>
          {customAvatarUrl && (
            <button
              onClick={() => { setAvatarChoice("custom"); saveProfile("custom"); }}
              title="Your uploaded photo"
              style={{
                width: 36, height: 36, borderRadius: "50%", cursor: "pointer", overflow: "hidden", padding: 0,
                border: avatarChoice === "custom" ? "2px solid var(--accent)" : "1px solid var(--border)",
              }}
            >
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img src={customAvatarUrl} alt="Your uploaded photo" width={36} height={36} style={{ objectFit: "cover" }} />
            </button>
          )}
          <label
            title="Upload a photo"
            style={{
              width: 36, height: 36, borderRadius: "50%", cursor: uploading ? "default" : "pointer", fontSize: 15,
              border: "1px dashed var(--border)", background: "var(--surface)",
              display: "flex", alignItems: "center", justifyContent: "center", opacity: uploading ? 0.5 : 1,
            }}
          >
            {uploading ? "…" : "+"}
            <input
              type="file"
              accept={ALLOWED_AVATAR_TYPES.join(",")}
              disabled={uploading}
              onChange={(e) => { const f = e.target.files?.[0]; if (f) onAvatarFileSelected(f); e.target.value = ""; }}
              style={{ display: "none" }}
            />
          </label>
          <button
            onClick={() => setAvatarChoice("google")}
            title="Your Google account photo"
            style={{
              width: 36, height: 36, borderRadius: "50%", cursor: "pointer", overflow: "hidden", padding: 0,
              border: avatarChoice === "google" ? "2px solid var(--accent)" : "1px solid var(--border)",
              display: "flex", alignItems: "center", justifyContent: "center", fontSize: 16, background: "var(--surface)",
            }}
          >
            {me?.profile?.google_avatar_url ? (
              // eslint-disable-next-line @next/next/no-img-element
              <img src={me.profile.google_avatar_url} alt="Google photo" width={36} height={36} style={{ objectFit: "cover" }} referrerPolicy="no-referrer" />
            ) : "G"}
          </button>
          {Object.entries(PRESETS).map(([key, p]) => (
            <button
              key={key}
              onClick={() => setAvatarChoice(key)}
              title={key}
              style={{
                width: 36, height: 36, borderRadius: "50%", cursor: "pointer", fontSize: 16, background: p.bg,
                border: avatarChoice === key ? "2px solid var(--accent)" : "1px solid var(--border)",
                display: "flex", alignItems: "center", justifyContent: "center",
              }}
            >
              {p.emoji}
            </button>
          ))}
          <button
            onClick={() => setAvatarChoice("initials")}
            title="Your initials"
            style={{
              width: 36, height: 36, borderRadius: "50%", cursor: "pointer", fontSize: 13, fontWeight: 600,
              background: "var(--accent)", color: "var(--on-accent, #fff)",
              border: avatarChoice === "initials" ? "2px solid var(--accent)" : "1px solid var(--border)",
              display: "flex", alignItems: "center", justifyContent: "center",
            }}
          >
            Aa
          </button>
        </div>
      </div>
      <p style={{ fontSize: 11, color: "var(--muted)", margin: "0 0 10px" }}>
        {uploadErr ? <span style={{ color: "var(--danger)" }}>{uploadErr}</span> : "PNG, JPEG, WEBP, or GIF — 2MB max."}
      </p>
      <textarea
        value={bio}
        onChange={(e) => setBio(e.target.value)}
        placeholder="A line about you — optional, up to 280 characters."
        maxLength={280}
        rows={2}
        style={{ width: "100%", padding: "8px 10px", fontSize: 14, borderRadius: 6, border: "1px solid var(--border)", background: "var(--bg)", color: "inherit", resize: "vertical" }}
      />
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 8 }}>
        <button onClick={() => saveProfile()} disabled={profileBusy} style={{ padding: "8px 14px", cursor: "pointer" }}>
          {profileBusy ? "Saving…" : "Save profile"}
        </button>
        {profileSaved && <span style={{ color: "var(--ok)", fontSize: 13 }}>Saved.</span>}
      </div>

      <h2 style={{ fontSize: 16, marginTop: 28 }}>Invite people</h2>
      <p style={{ color: "var(--muted-strong)", fontSize: 13 }}>Each code works 5 times for 30 days. The Hive is invite-only — hand these to people you'd vouch for.</p>
      <div style={{ display: "flex", gap: 8 }}>
        <input value={note} onChange={(e) => setNote(e.target.value)} placeholder="note to self, e.g. “Tuesday crew”" style={{ flex: 1, padding: 8 }} />
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
        The project chat runs on a frontier model. With your own key it runs on your account and costs the Hive nothing;
        without one, the hub's key is used and charged to your purchased Honey. Keys are stored encrypted (Supabase Vault) and only ever read by that chat.
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
          <>
            <code style={{ fontSize: 15, padding: "6px 10px", background: "var(--surface)", border: "1px solid var(--border)", borderRadius: 6 }}>
              /link {linkCode}
            </code>
            <button onClick={copyLinkCommand} style={{ padding: "8px 14px", cursor: "pointer" }} aria-label="Copy link command">
              {linkCopied ? "Copied!" : "Copy"}
            </button>
          </>
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
