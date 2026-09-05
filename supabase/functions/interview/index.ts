// Interviewer agent Edge Function (ADR-006 D37–D39). STUB.
//
// Contract: POST { member_id, conversation: [{role, content}] }
//   → { reply: string, plan?: ProjectPlan }   (plan present when the interview is complete)
// Charges `spend_interview` per token (ADR-002 §7) via hive.post_txn RPC — not yet written.
// Must ask about: internet need (D47) and license (D54). Output validates against
// packages/schema/project-plan.schema.json before any row is written.

Deno.serve(async (req) => {
  if (req.method !== "POST") return new Response("POST only", { status: 405 });
  return Response.json(
    { error: "not_implemented", see: "Cmd Work: Build the interviewer agent Edge Function" },
    { status: 501 },
  );
});
