import type { CSSProperties } from "react";

// Server/node avatars (Jack, 2026-09-12): "for the people who run servers, servers should be able
// to get server avatars, we'll include presets like Mac Minis, Mac Studios, iMacs, and a rack
// mounted server image." No real emoji fits this hardware, so these are small flat SVG glyphs
// (rounded-square badge, not a circle) -- deliberately distinct from the round member Avatar so
// the two never get confused at a glance in the same list.

export const NODE_PRESETS: Record<string, { label: string; bg: string }> = {
  mac_mini: { label: "Mac Mini", bg: "#8E8B99" },
  mac_studio: { label: "Mac Studio", bg: "#B9B6C3" },
  imac: { label: "iMac", bg: "#6FB3D9" },
  rack_server: { label: "Rack server", bg: "#4A4850" },
};

function Glyph({ choice }: { choice: string }) {
  const common = { width: "58%", height: "58%", fill: "none", stroke: "#fff", strokeWidth: 1.6, strokeLinecap: "round" as const, strokeLinejoin: "round" as const };
  switch (choice) {
    case "mac_mini":
      // small flat box, seen from a slight angle
      return <svg viewBox="0 0 24 24" style={common}><rect x="3" y="8" width="18" height="10" rx="1.5" /><path d="M3 11h18" /></svg>;
    case "mac_studio":
      // squat cylinder
      return <svg viewBox="0 0 24 24" style={common}><rect x="5" y="5" width="14" height="14" rx="4" /><path d="M5 12h14" /></svg>;
    case "imac":
      // monitor + stand
      return <svg viewBox="0 0 24 24" style={common}><rect x="3" y="3" width="18" height="13" rx="1.5" /><path d="M12 16v4M8 21h8" /></svg>;
    case "rack_server":
      // 1U rack unit strip
      return (
        <svg viewBox="0 0 24 24" style={common}>
          <rect x="3" y="4" width="18" height="16" rx="1.5" />
          <path d="M6 8h.01M6 12h.01M6 16h.01M10 8h8M10 12h8M10 16h8" />
        </svg>
      );
    default:
      // generic node/chip glyph -- "auto", nothing chosen yet
      return (
        <svg viewBox="0 0 24 24" style={common}>
          <rect x="6" y="6" width="12" height="12" rx="2" />
          <path d="M9 3v3M15 3v3M9 18v3M15 18v3M3 9h3M3 15h3M18 9h3M18 15h3" />
        </svg>
      );
  }
}

export function NodeAvatar({
  choice, role, size = 40,
}: { choice: string | null | undefined; role?: string | null; size?: number }) {
  // 'auto' (or unset) picks a sensible default from role rather than showing a blank box.
  const effective = choice && choice !== "auto" ? choice : role === "regional_server" || role === "compute_and_server" ? "rack_server" : "auto";
  const bg = NODE_PRESETS[effective]?.bg ?? "#54525C";
  const box: CSSProperties = {
    width: size, height: size, borderRadius: size * 0.28, flexShrink: 0, background: bg,
    display: "flex", alignItems: "center", justifyContent: "center", border: "1px solid var(--border)",
  };
  return (
    <div style={box} title={NODE_PRESETS[effective]?.label ?? "Node"} aria-label={NODE_PRESETS[effective]?.label ?? "Node"}>
      <Glyph choice={effective} />
    </div>
  );
}
