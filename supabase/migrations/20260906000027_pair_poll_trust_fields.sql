-- Hive — pair_poll returns trust flags at claim (2026-09-06).
--
-- Bug found while wiring the desktop Trust switches: crates/hive and the desktop app hardcode
-- allow_internet/tools_level when building Capabilities, so hive.node_checkin's
-- coalesce((p_capabilities->>'allow_internet')::boolean, allow_internet) never falls through to
-- the existing row -- any trust setting chosen at pairing (web /pair) is silently reset to the
-- default on the node's first check-in/heartbeat. This migration is the hub-side half of the
-- fix: let a freshly-paired node learn what was actually chosen so it can seed its local
-- node.env before ever calling check-in. The node-agent fix lands in the same change set.

create or replace function hive.pair_poll(p_secret text)
returns jsonb language plpgsql security definer set search_path = hive, public, extensions as $$
declare r hive.pairings; h text; out jsonb; n hive.nodes; begin
  h := encode(extensions.digest(p_secret::bytea, 'sha256'), 'hex');
  select * into r from hive.pairings where secret_hash = h;
  if not found then return jsonb_build_object('status', 'expired'); end if;
  if r.expires_at < now() and r.raw_key is null then
    delete from hive.pairings where code = r.code; return jsonb_build_object('status', 'expired');
  end if;
  if r.raw_key is null then return jsonb_build_object('status', 'pending'); end if;
  select * into n from hive.nodes where id = r.node_id;
  out := jsonb_build_object('status', 'claimed', 'node_key', r.raw_key, 'node_id', r.node_id,
                            'display_name', n.display_name, 'allow_internet', n.allow_internet,
                            'tools_level', n.tools_level);
  delete from hive.pairings where code = r.code;     -- one-shot
  return out;
end $$;

grant execute on function hive.pair_poll(text) to anon, authenticated, service_role;
