-- Audit S-5. Keep explicitly granted API endpoints; remove implicit PUBLIC reachability.
-- SECURITY DEFINER wrappers execute their internal calls as their owner, not the caller.
do $permissions$
declare f record; owner_name text; col record;
begin
  for f in select p.oid::regprocedure as signature from pg_proc p join pg_namespace n on n.oid=p.pronamespace
           where p.prokind='f' and (n.nspname='hive' or (n.nspname='public' and p.proname like 'hive\_%' escape '\'))
  loop
    execute format('revoke execute on function %s from public',f.signature);
  end loop;

  -- These helpers accept another member's identity or perform privileged maintenance.
  -- Deny even if a deployment accidentally added explicit client grants.
  for f in select p.oid::regprocedure as signature from pg_proc p join pg_namespace n on n.oid=p.pronamespace
           where n.nspname='hive' and p.prokind='f' and p.proname in
             ('member_key_set_for','member_key_remove_for','settle_storage','retire_old_backups')
  loop
    execute format('revoke execute on function %s from anon, authenticated',f.signature);
  end loop;

  -- PUBLIC function execution is a GLOBAL default: schema-only REVOKE cannot remove it.
  -- Cover existing Hive function owners, not merely the current migration login.
  for owner_name in select distinct pg_get_userbyid(p.proowner) from pg_proc p join pg_namespace n on n.oid=p.pronamespace
                    where n.nspname='hive' or (n.nspname='public' and p.proname like 'hive\_%' escape '\')
  loop
    execute format('alter default privileges for role %I revoke execute on functions from public, anon, authenticated',owner_name);
    execute format('alter default privileges for role %I in schema hive, public revoke execute on functions from public, anon, authenticated',owner_name);
    execute format('alter default privileges for role %I in schema hive revoke select on tables from authenticated',owner_name);
  end loop;

  revoke insert,update,delete,truncate,references,trigger on hive.members from public,anon,authenticated;
  -- A table REVOKE does not cancel independently granted column permissions.
  for col in select attname from pg_attribute where attrelid='hive.members'::regclass and attnum>0 and not attisdropped
  loop
    execute format('revoke insert (%I),update (%I),references (%I) on hive.members from public,anon,authenticated',col.attname,col.attname,col.attname);
  end loop;
  revoke all on hive.balances from public,anon,authenticated;
end $permissions$;

-- Read-only identity/visibility helpers needed when RLS runs as authenticated users.
-- Each inspects auth.uid(); none permits caller-selected member impersonation or mutation.
grant execute on function hive.is_member() to authenticated;
grant execute on function hive.project_role(uuid) to authenticated;
grant execute on function hive.is_project_admin(uuid) to authenticated;
grant execute on function hive.project_visible(uuid) to authenticated;
grant execute on function hive.card_visible(uuid) to authenticated;

-- Existing public wrappers retain their explicit anon/authenticated/service_role grants.
-- Profile edits must go through hive_member_update_profile / other validated RPCs.
-- Account balances remain available through the existing owner-checked wallet/board RPCs.

-- Fail the migration rather than silently accept a deployment-specific inherited grant.
do $verify_permissions$
declare f record; client_role text; col record;
begin
  foreach client_role in array array['anon','authenticated'] loop
    for f in select p.oid from pg_proc p join pg_namespace n on n.oid=p.pronamespace
             where n.nspname='hive' and p.proname in ('member_key_set_for','member_key_remove_for','settle_storage','retire_old_backups')
    loop
      if has_function_privilege(client_role,f.oid,'EXECUTE') then
        raise exception 'unexpected inherited client privilege on protected Hive helper';
      end if;
    end loop;
    for col in select attnum from pg_attribute where attrelid='hive.members'::regclass and attnum>0 and not attisdropped loop
      if has_column_privilege(client_role,'hive.members',col.attnum,'UPDATE')
         or has_column_privilege(client_role,'hive.members',col.attnum,'INSERT') then
        raise exception 'unexpected inherited client write privilege on Hive members';
      end if;
    end loop;
    if has_table_privilege(client_role,'hive.balances','SELECT') then
      raise exception 'unexpected inherited client read privilege on Hive balances';
    end if;
  end loop;
end $verify_permissions$;
