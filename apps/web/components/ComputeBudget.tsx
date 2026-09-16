"use client";

import { useEffect, useState } from "react";
import { supabaseBrowser } from "@/lib/supabase";

type Budget = {
  can_approve: boolean; approved: boolean; valid: boolean; funded: boolean;
  max_honey: number | null; spent: number; remaining: number | null; reserved: number;
  payer: "member_wallet" | "project_fund";
};
const amount = (n: number | null) => Number(n ?? 0).toLocaleString(undefined, { maximumFractionDigits: 6 });

export function ComputeBudget({ cardId, cardStatus }: { cardId: string; cardStatus: string }) {
  const [budget, setBudget] = useState<Budget | null>(null);
  const [limit, setLimit] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [refresh, setRefresh] = useState(0);
  useEffect(() => {
    let active = true;
    setBudget(null);
    setError(null);
    Promise.resolve(supabaseBrowser().rpc("hive_compute_budget_status", { p_card_id: cardId })).then(({ data, error }) => {
      if (!active) return;
      if (error) setError(error.message); else setBudget(data as Budget);
    }).catch(() => { if (active) setError("Could not load this budget. Please refresh."); });
    return () => { active = false; };
  }, [cardId, cardStatus, refresh]);

  async function approve() {
    if (busy || !budget?.can_approve) return;
    const value = Number(limit);
    if (!Number.isFinite(value) || value <= 0) return;
    setBusy(true); setError(null);
    try {
      const { error } = await supabaseBrowser().rpc("hive_compute_budget_approve", { p_card_id: cardId, p_max_honey: value });
      if (error) setError(error.message);
      else setRefresh((n) => n + 1);
    } catch { setError("Could not save the limit. Refresh to check whether it was saved before trying again."); }
    finally { setBusy(false); }
  }
  return (
    <section aria-label="Job Honey budget" style={{ marginTop: 12, padding: 10, border: "1px solid var(--border)", borderRadius: 6 }}>
      <strong>Honey limit</strong>
      {error && <p role="alert">{error}</p>}
      {!budget && !error && <p role="status">Loading budget…</p>}
      {budget && <>
        {budget.approved ? <>
          <p>Approved: {amount(budget.max_honey)} · Spent: {amount(budget.spent)} · Remaining: {amount(budget.remaining)} · Reserved: {amount(budget.reserved)}</p>
          <p>{cardStatus === "done" || cardStatus === "review" ? "This attempt has finished. The totals above include its payment." : !budget.valid ? "This approval no longer matches the job. It cannot run with this approval."
            : Number(budget.remaining) <= 0 ? "The approved budget has been used. Further work needs a new approval."
            : budget.reserved > 0 ? "Funds are reserved for this job."
            : budget.funded ? "Funds are available. The job can be picked up when it is ready."
            : `Waiting for enough Honey in the ${budget.payer === "member_wallet" ? "requester's wallet" : "project fund"}.`}</p>
          <p>Retries share this limit; they do not renew it.</p>
        </> : <p>The owner must approve a spending limit before this job can run.</p>}
        {budget.can_approve && <form onSubmit={(e) => { e.preventDefault(); void approve(); }}>
          <label>Maximum Honey for this job
            <input type="number" min="0.000001" step="0.000001" required value={limit}
              onChange={(e) => setLimit(e.target.value)} disabled={busy} style={{ width: "100%" }} />
          </label>
          <p>This authorizes spending from the project fund, including retries. Funds are reserved when a worker picks up the job.</p>
          <button disabled={busy || !Number.isFinite(Number(limit)) || Number(limit) <= 0} type="submit">{busy ? "Saving…" : "Approve limit"}</button>
        </form>}
      </>}
      <button type="button" disabled={busy} onClick={() => setRefresh((n) => n + 1)}>Refresh budget</button>
    </section>
  );
}
