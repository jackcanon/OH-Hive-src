> Completed: migration applied, bounded WAN validation passed, temporary access cleaned up. See [WAN results](SIF-DELEGATION-WAN-RESULTS-2026-09-13.md). Earlier pending-approval text below is historical.

# Delegation live validation — execution approval

Claude reviewed the SQL and logged Jack's approval. Automatic approval review rejected execution
before any command ran because approval was relayed through the continuity log rather than given
directly in this conversation. A subsequent read-only query confirmed: delegation table absent,
pilot allowlist disabled, restricted pilot login disabled.

The reviewed SQL is now at `supabase/migrations/20260914030000_control_pilot_delegation.sql`.
SHA-256: `843ed121c8cdf13a032912c45f5b5ce8bd8ce9195dca330673baa3e33e6b5900`.
It was moved unchanged locally; it has not been applied or recorded in the live database.

Requested execution scope:

1. Apply and record this exact reviewed delegation migration.
2. Temporarily enable `hive_ctl_chicago_pilot` with a fresh password, retain its existing bounded
   gateway grants, and add only the readiness-function grant. Enable only its existing Chicago /
   duplicate-project allowlist (`6045a447-04ae-4a2d-bf7a-860c3d63257b`).
3. Issue fresh keys for the two existing temporary pilot workers, then issue their scoped
   delegations directly at the trusted authority. No normal worker credentials are used.
4. Start the isolated Chicago pilot on loopback 8791 and temporarily restore only its
   `/hive/ctl/1/` HTTPS route. Test authentication, readiness, token renewal/re-authentication and
   recovery; interrupt only the pilot database connection/process to measure recovery.
5. Stop the pilot, remove its route/credential file, revoke test keys/delegations/tokens,
   disable pilot login/allowlist, and verify original service health and direct RPC.

No new Honey allocation, community cutover, normal worker changes, or original-project edits.
The migration persists; test credentials and routing are temporary. Existing completed outputs
and the duplicate project remain for review. Results go in continuity signed by Sif.
