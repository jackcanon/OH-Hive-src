// Single source of truth for the Terms of Service version. Bump the date whenever the terms
// text in app/terms/page.tsx materially changes; hive.invite_redeem() stamps this onto new
// members via hive.members.tos_version (see supabase/migrations/20260910000500_tos_acceptance.sql).
export const TOS_VERSION = "2026-09-10";
