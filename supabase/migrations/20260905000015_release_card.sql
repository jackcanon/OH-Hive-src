-- Hive — node_release_card: a node hands a leased card back without failing it. Safe to re-run.
-- Card → ready, lease dropped, checkpoints kept so the next claimant resumes (ADR-006 D42).
-- Used on graceful shutdown (Ctrl-C / systemctl stop) mid-card.
create or replace function hive.node_release_card(raw_key text, p_card_id uuid, p_reason text default '')
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  delete from hive.leases where card_id = p_card_id and node_id = nid;
  if not found then return jsonb_build_object('status', 'no_lease'); end if;
  update hive.cards set status = 'ready' where id = p_card_id and status = 'running';
  return jsonb_build_object('status', 'released', 'card_id', p_card_id, 'reason', p_reason,
                            'checkpoint_step', (select max(step) from hive.checkpoints where card_id = p_card_id));
end $$;
create or replace function public.hive_node_release_card(raw_key text, p_card_id uuid, p_reason text default '') returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.node_release_card(raw_key, p_card_id, p_reason); $$;
grant execute on function public.hive_node_release_card(text, uuid, text) to anon, authenticated;
