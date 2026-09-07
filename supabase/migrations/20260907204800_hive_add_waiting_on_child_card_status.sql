-- ADR-006 D44 sub-delegation pause/resume: a new status for a card that has spawned a
-- child and released its lease to wait for it, distinct from 'blocked' (which
-- hive.node_fail_card already uses to mean "failed, needs attention" -- reusing it here
-- would have made a waiting card look like a failed one).
alter type hive.card_status add value if not exists 'waiting_on_child' after 'blocked';
