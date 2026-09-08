/**
 * Maps raw Postgres/RPC error codes (snake_case exception messages from `hive.*` functions) to
 * copy a first-time, non-technical user can actually understand. Jack, 2026-09-08: friends are
 * about to test this live — a leaked `insufficient_honey` or `overflow_unavailable` reads as
 * broken, not as "the system is telling you something."
 *
 * Usage: `friendlyError(error.message)` wherever an RPC error is shown to a user. Falls back to a
 * de-snake-cased, capitalized version of anything not in the map, so unknown codes stay readable
 * instead of silently swallowed.
 */
const KNOWN: Record<string, string> = {
  insufficient_honey: "You don't have enough Honey for that.",
  insufficient_honey_in_sources: "You don't have enough Honey of the right kind for that yet.",
  overflow_unavailable:
    "The Hive's cloud budget for this is used up right now. Try again later, or use your own API key in Settings.",
  hub_not_configured: "The Hive's cloud interviewer isn't set up right now — try again in a bit.",
  not_a_hive_member: "You need to join the Hive first.",
  not_a_member: "You need to join the Hive first.",
  project_not_found: "That project doesn't exist, or you don't have access to it.",
  amount_must_be_positive: "Enter an amount greater than zero.",
  no_wallet_for_member: "You don't have a Hive wallet yet — join the Hive first.",
  already_member: "You're already a member of the Hive.",
  code_expired: "That code has expired — ask for a new one.",
  code_not_found: "That code isn't valid. Double-check it and try again.",
  invalid_invite: "That invite code isn't valid. Double-check it and try again.",
  turn_failed: "That didn't go through. Try sending it again.",
  no_node_online: "No machine is online to run this right now.",
  invite_not_found: "That invite is already gone — it may have just been revoked in another tab.",
  not_project_admin: "You need to be a project admin to do that.",
  not_project_owner: "Only the project owner can do that.",
  empty_comment: "Write something before posting.",
  comment_too_long: "That comment is too long (max 8,000 characters).",
  parent_comment_not_found: "That comment has been deleted and can't be replied to anymore.",
  not_found_or_not_yours: "You can only edit your own comments.",
  not_allowed: "You don't have permission to do that.",
  not_found: "That comment isn't there anymore.",
};

export function friendlyError(raw: string | null | undefined): string {
  if (!raw) return "Something went wrong. Try again.";
  const key = raw.trim().toLowerCase();
  if (KNOWN[key]) return KNOWN[key];
  // Unknown code: de-snake-case and capitalize so it's at least readable, never raw jargon.
  const cleaned = raw.replace(/_/g, " ").trim();
  if (!cleaned) return "Something went wrong. Try again.";
  return cleaned.charAt(0).toUpperCase() + cleaned.slice(1) + (/[.!?]$/.test(cleaned) ? "" : ".");
}
