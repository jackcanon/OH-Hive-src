-- Audit S-1: failing a card requires owning its current lease. Raising rolls back all effects.
create or replace function hive.node_fail_card(raw_key text, p_card_id uuid, p_reason text)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  delete from hive.leases where card_id = p_card_id and node_id = nid;
  if not found then raise exception 'no_owned_lease'; end if;
  update hive.cards set status = 'blocked' where id = p_card_id;
  insert into hive.card_outputs (card_id, node_id, content, usage) values (p_card_id, nid, 'FAILED: ' || p_reason, '{}'::jsonb);
  return jsonb_build_object('status', 'blocked');
end $$;
