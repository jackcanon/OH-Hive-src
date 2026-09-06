// The Honey currency symbol — docs/honey-logo-package/masters/svg/honey-symbol-gold.svg, inlined
// (the guide: a bold H crossed by one continuous vertical stroke; keep ≥ 80 units clear space, never
// rotate/stretch). Rendered at text height next to amounts; the color comes from --gold in globals.css.

export function HoneyMark({ height = "1em", title = "Honey" }: { height?: string | number; title?: string }) {
  return (
    <svg viewBox="0 0 600 656" role="img" aria-label={title} style={{ height, width: "auto" }} xmlns="http://www.w3.org/2000/svg">
      <path style={{ fill: "var(--gold)" }} d="M120 144 L200 144 L200 288 L284 288 L284 80 L316 80 L316 288 L400 288 L400 144 L480 144 L480 512 L400 512 L400 368 L316 368 L316 576 L284 576 L284 368 L200 368 L200 512 L120 512 Z" />
    </svg>
  );
}

export function formatHoney(n: number | string | null | undefined, digits = 4) {
  return Number(n ?? 0).toLocaleString(undefined, { maximumFractionDigits: digits });
}

/** An amount of Honey: gold H symbol + number. `<Honey n={12.5} />` → ⟨H⟩ 12.5 */
export function Honey({ n, digits = 4, suffix = false }: { n: number | string | null | undefined; digits?: number; suffix?: boolean }) {
  return (
    <span className="honey" title={`${formatHoney(n, digits)} Honey`}>
      <HoneyMark />
      {formatHoney(n, digits)}
      {suffix && <span style={{ color: "var(--muted)" }}>Honey</span>}
    </span>
  );
}
