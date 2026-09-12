import type { CSSProperties } from "react";

// Member avatar (Jack, 2026-09-12): "there is an avatar that people can select in their own bio,
// and by default it will scrape the Google account photo." The Google-photo scrape is infra this
// Supabase project already has (public.profiles.avatar_url gets populated on Google sign-in) --
// this component just decides what to render for a given member's `avatar_choice` + that URL, and
// the picker in Settings shares the same PRESETS so the two stay in sync automatically.
//
// No image hosting for the presets on purpose -- they're emoji-on-a-color-circle, all client-side.

export const PRESETS: Record<string, { emoji: string; bg: string }> = {
  bee: { emoji: "🐝", bg: "#f5c211" },
  wolf: { emoji: "🐺", bg: "#5b6b73" },
  raven: { emoji: "🐦", bg: "#2b2b33" },
  fox: { emoji: "🦊", bg: "#d9743a" },
  owl: { emoji: "🦉", bg: "#7a5c3e" },
  bear: { emoji: "🐻", bg: "#6b4a35" },
};

function initials(name: string) {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "?";
  return (parts[0][0] + (parts[1]?.[0] ?? "")).toUpperCase();
}

export function Avatar({
  choice, googleUrl, name, size = 40,
}: { choice: string | null | undefined; googleUrl: string | null | undefined; name: string; size?: number }) {
  const circle: CSSProperties = {
    width: size, height: size, borderRadius: "50%", flexShrink: 0,
    display: "flex", alignItems: "center", justifyContent: "center",
    fontSize: size * 0.55, overflow: "hidden", border: "1px solid var(--border)",
  };

  if ((!choice || choice === "google") && googleUrl) {
    // eslint-disable-next-line @next/next/no-img-element -- external Google-hosted URL, not a local asset
    return <img src={googleUrl} alt={name} width={size} height={size} style={{ ...circle, objectFit: "cover" }} referrerPolicy="no-referrer" />;
  }
  if (choice && choice !== "google" && choice !== "initials" && PRESETS[choice]) {
    const p = PRESETS[choice];
    return <div style={{ ...circle, background: p.bg }} title={choice} aria-label={choice}>{p.emoji}</div>;
  }
  return (
    <div style={{ ...circle, background: "var(--accent)", color: "var(--on-accent, #fff)", fontWeight: 600, fontSize: size * 0.4 }} title={name}>
      {initials(name)}
    </div>
  );
}
