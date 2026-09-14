-- Task #188 root cause (found live, 2026-09-13 21:46 UTC investigation): node_complete_card has
-- never set `source` on its ledger entries. hive.ledger_entries.source is NOT NULL and
-- hive.post_txn explicitly raises 'ledger_entry_missing_source' when it's absent -- so ANY card
-- completing with amt > 0 (i.e. every non-'code' card that earned honey) has had its completion
-- roll back since at least 2026-09-11T20:25 UTC. The worker had already done the real work and
-- correctly called complete_card, but the RPC itself failed atomically: the lease was never
-- released and the card never left 'running', so it sat until its lease naturally expired, got
-- reclaimed, redid the same work, and hit the same error again -- the "~16-minute crash loop"
-- symptom was this retry cycle, not a process crash (confirmed live: same PID stable for hours).
--
-- Fix has two parts:
--  1. Route the fund's debit through hive.split_debit (the function this morning's payout-cap
--     fix already noted this should have been using) instead of a raw debit with no source --
--     split_debit tags each tranche with its real source (earned/grant/purchased) and caps to
--     what's actually available there, which also makes the earlier ad hoc
--     `amt := least(amt, fund_balance)` clamp redundant (kept below as a defensive pre-check so
--     split_debit is never asked to cover more than the fund's total balance).
--  2. Never let a ledger-posting failure orphan already-completed work again: if split_debit or
--     post_txn raises for any reason, the card still needs to leave 'running' and its lease still
--     needs to release -- that's what "the node reported completion" means. Pay 0 honey and log a
--     warning rather than repeat this exact multi-day incident for the next edge case no one has
--     hit yet.
create or replace function hive.node_complete_card(raw_key text, p_card_id uuid, p_content text, p_model_id text, p_tokens_in bigint, p_tokens_out bigint, p_compute_seconds numeric)
returns jsonb
language plpgsql
security definer
set search_path to 'hive', 'public'
as $function$
declare nid uuid; l hive.leases; c hive.cards; fund uuid; wallet uuid; r_out hive.rate_table; r_in hive.rate_table;
        amt numeric; fund_balance numeric; tid uuid; p_owner uuid; p_title text; debits jsonb; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into l from hive.leases where card_id = p_card_id and node_id = nid;
  if not found then raise exception 'no_lease_for_this_node'; end if;
  select * into c from hive.cards where id = p_card_id;

  insert into hive.card_outputs (card_id, node_id, content, model_id, usage)
  values (p_card_id, nid, p_content, p_model_id,
          jsonb_build_object('tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds));

  select fund_account_id into fund from hive.projects where id = c.project_id;
  select a.id into wallet from hive.accounts a join hive.nodes n on n.member_id = a.member_id where n.id = nid and a.kind = 'member_wallet';
  r_out := hive.current_rate('compute_output'); r_in := hive.current_rate('compute_input');
  amt := round(coalesce(p_tokens_out,0) * r_out.honey_per_unit + coalesce(p_tokens_in,0) * coalesce(r_in.honey_per_unit,0), 6);

  fund_balance := greatest(hive.account_balance(fund), 0);
  amt := least(amt, fund_balance);

  if amt > 0 then
    begin
      debits := hive.split_debit(fund, amt, array['earned','grant','purchased'],
                  jsonb_build_object('entry_type', 'spend_job', 'rate_id', r_out.id, 'tokens_in', p_tokens_in,
                                      'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds,
                                      'card_id', p_card_id, 'node_id', nid));
      tid := hive.post_txn(debits || jsonb_build_array(
        jsonb_build_object('account_id', wallet, 'entry_type', 'earn_compute', 'direction', 'credit', 'amount', amt, 'source', 'earned', 'rate_id', r_out.id,
                           'tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds, 'card_id', p_card_id, 'node_id', nid)
      ), 'card ' || c.key || ' on ' || (select display_name from hive.nodes where id = nid));
    exception when others then
      raise warning 'node_complete_card: ledger post failed for card % (paying 0 honey instead of leaving it stuck): %', p_card_id, sqlerrm;
      amt := 0; tid := null;
    end;
  end if;

  delete from hive.leases where card_id = p_card_id;
  update hive.cards set status = 'review' where id = p_card_id;

  select owner_id, title into p_owner, p_title from hive.projects where id = c.project_id;
  insert into hive.notification_events (event_type, project_id, card_id, member_id, payload)
  values ('card_completed', c.project_id, p_card_id, p_owner,
          jsonb_build_object('card_title', c.title, 'project_title', p_title, 'earned_honey', amt, 'node_region', (select region from hive.nodes where id = nid)));

  return jsonb_build_object('status', 'review', 'earned_honey', amt, 'txn_id', tid,
                            'fund_balance', hive.account_balance(fund), 'wallet_balance', hive.account_balance(wallet));
end $function$;
