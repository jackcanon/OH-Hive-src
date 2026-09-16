# Audit S-5: database permission boundary

Forward migration: `supabase/migrations/20260915170000_hive_permissions_boundary.sql`. Implemented and fixture-tested locally; not applied to production.

## Changes

- Removes PUBLIC execute from existing `hive` functions and `public.hive_*` wrappers. Existing explicit endpoint grants to anon/authenticated/service_role remain intact, including node-key APIs and service-only endpoints. Unrelated existing public-schema functions are untouched.
- Explicitly removes anon/authenticated access to every overload of `member_key_set_for`, `member_key_remove_for`, `settle_storage`, and `retire_old_backups`, even if a deployment added direct grants. Definer wrappers and owner-run maintenance still work.
- Removes direct member-table writes, including independently granted column permissions. A user's own row no longer grants the ability to set `is_admin`, change status, or bypass validated profile RPCs. Existing SELECT/RLS policies remain. Profile/avatar/registration changes continue through their existing security-definer RPCs.
- Revokes client access to `hive.balances`. It is a view, so enabling RLS on it is not a valid repair. Wallet and board RPCs retain owner-context access. No checked-in app caller directly selects this view.
- Grants authenticated execution of the five read-only RLS predicates: is_member, project_role, is_project_admin, project_visible, card_visible.
- Changes defaults for current Hive function owners so newly created functions no longer automatically grant PUBLIC/anon/authenticated execution. Global defaults must be revoked as well as schema defaults; a schema-only revoke cannot override PostgreSQL's global PUBLIC default. Removes future authenticated blanket SELECT for Hive tables.
- Checks effective privileges after changes and aborts if inherited client roles still grant dangerous helper access, member writes, or balances reads.

## Compatibility and deployment review

All 132 public Hive wrapper names found in the repository have explicit grant statements and SECURITY DEFINER declarations. This source check is not proof of production ACLs or a review of every endpoint's internal authorization. Existing explicit grants are intentionally preserved, including grants that may have originated from older role defaults; before rollout review actual endpoint ACLs against intended repository grants. Service-role permissions are not widened. Existing helper invocations inside definer wrappers run as the wrapper owner.

This migration must run with rights to alter defaults for every discovered Hive function owner. It fails rather than silently skipping an owner it cannot manage. Future migration authors must explicitly grant intended endpoints after creation, including new functions in other schemas owned by those same roles: global default PUBLIC execution cannot be removed for only one schema. Existing unrelated functions keep their ACLs. Future Hive tables require intentional read grants and RLS. Future functions created by an entirely new owner require that owner's defaults to be hardened as well.

Apply via the normal transactional migration process; do not paste selected statements independently. Before deployment capture the function grants/default grants and verify production-only wrappers and policy helpers, scheduled jobs, custom roles and any external direct-table clients. In particular, independently written clients that update members directly or read balances must switch to the validated RPCs. Do not restore blanket grants as a workaround. No production ACL inspection or full migration replay is claimed.

Useful production review queries (read-only):

```sql
select p.oid::regprocedure as function, pg_get_userbyid(p.proowner) as owner,
       has_function_privilege('anon',p.oid,'EXECUTE') as anon,
       has_function_privilege('authenticated',p.oid,'EXECUTE') as authenticated,
       has_function_privilege('service_role',p.oid,'EXECUTE') as service_role,
       p.proacl
from pg_proc p join pg_namespace n on n.oid=p.pronamespace
where n.nspname='hive' or (n.nspname='public' and p.proname like 'hive\_%' escape '\')
order by 1;
select * from pg_default_acl;
```

## Verification

`scripts/test-hive-permissions.mjs` uses PGlite/PostgreSQL with actual role switching, RLS, explicit column grants, a balances view and the actual member_update_profile RPC body. Checks that self-admin updates, direct insert/status writes, foreign-key-secret helpers, maintenance calls and direct balance reads fail. Confirms own-profile edits, node-key endpoints, service-only calls, policy reads and owner-only nested key writes continue working. Tests new-function/table defaults (including Supabase-style schema-level anon/authenticated execution defaults) and verifies inherited member-write grants cause migration failure. Other endpoint bodies are narrow fixtures, not production schema replay.

Static migration guards and diff checks also pass. Next assigned audit work remains S-6 debit/lease row locking. Ledger integrity and broader endpoint authorization issues are not solved by ACL changes alone.
