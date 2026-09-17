-- Put cloud-brain token prices in `hive.settings` so they can change without redeploying an Edge
-- Function, and seed real figures for openai and nous.
--
-- Why this exists. 20260916060000 shipped the spend meter with prices compiled into
-- `code-brain-turn`: anthropic hard-coded, openai and nous deliberately UNPRICED because I had no
-- verified figures and a wrong price is worse than a missing one. The consequence was stated at the
-- time and is the problem: an unpriced provider records real token counts at a $0 estimate, so it
-- never counts against the monthly ceiling. Jack then said he expects OpenAI or Nous to be the
-- providers actually driving the coding loop -- which turns "unpriced" from a footnote into a hole
-- straight through the cap.
--
-- Env vars would have worked (`CODE_BRAIN_PRICE_OPENAI="1.25,10"`) but every repricing is then a
-- function redeploy. The monthly ceiling already lives in `hive.settings`
-- (`code_brain_usd_cap_month`), so prices belong beside it: one UPDATE, no deploy, and the two
-- numbers that decide whether the cap binds are readable in the same place.
--
-- PER-MODEL, NOT JUST PER-PROVIDER, because within one provider the spread is large enough to break
-- a ceiling: gpt-5 is $1.25/$10 per Mtok while gpt-5.5 is $5/$30 -- four times the input cost and
-- three times the output. A per-provider price that happens to be the cheap model silently
-- under-counts every turn. The `models` map is checked first by exact model id, with the provider
-- entry as the fallback.
--
-- FIGURES AND WHERE THEY CAME FROM (2026-09-17, from each vendor or a vendor-pricing aggregator --
-- re-check before trusting them for billing, since none of this is contractual):
--
--   anthropic  3.00 / 15.00   claude-sonnet-4-5. Not new: this is the pair
--                             `supabase/functions/interview/index.ts:78` has always priced the
--                             interviewer at, carried over rather than invented.
--   openai     1.25 / 10.00   gpt-5, which is `code-brain-turn`'s configured default for openai.
--                             gpt-5.2 (1.75/14) and gpt-5.5 (5/30) are in the models map so that
--                             switching model does not silently switch off the cap.
--   nous       3.00 / 15.00   DELIBERATELY OVER-ESTIMATED, and this is the one judgement call
--                             here. `code-brain-turn`'s configured nous default is
--                             `anthropic/claude-sonnet-4.6` -- a Claude-class model reached through
--                             Nous Portal -- so it is priced at Claude rates. A ceiling should err
--                             HIGH: over-estimating spends the cap early, under-estimating means
--                             the cap silently does not bind, and only one of those failures is
--                             recoverable. If nous is switched to a Hermes model the real rate is
--                             far lower (Hermes-4-405B around 0.09/0.37, Hermes-4-70B around
--                             0.05/0.2 per public listings) and this should be lowered -- both are
--                             in the models map for exactly that case.
--
-- One thing to verify that I could not: a public model listing for Nous Portal shows only the two
-- Hermes models and does NOT list `anthropic/claude-sonnet-4.6`. If that listing is complete, the
-- configured nous default does not exist on that endpoint and a nous code card would fail on a
-- model-not-found rather than on price. I have no nous key to test with and would not spend on one
-- unasked, so this is flagged rather than concluded.
--
-- Changing a price later:
--   update hive.settings
--      set value = jsonb_set(value, '{openai,in}', '2.0'::jsonb), updated_at = now()
--    where key = 'code_brain_prices';
-- Nothing needs redeploying; the Edge Function reads this per request.
begin;

insert into hive.settings (key, value) values (
  'code_brain_prices',
  jsonb_build_object(
    'anthropic', jsonb_build_object('in', 3.0, 'out', 15.0),
    'openai',    jsonb_build_object('in', 1.25, 'out', 10.0, 'models', jsonb_build_object(
                   'gpt-5',   jsonb_build_object('in', 1.25, 'out', 10.0),
                   'gpt-5.1', jsonb_build_object('in', 1.25, 'out', 10.0),
                   'gpt-5.2', jsonb_build_object('in', 1.75, 'out', 14.0),
                   'gpt-5.5', jsonb_build_object('in', 5.0,  'out', 30.0))),
    'nous',      jsonb_build_object('in', 3.0, 'out', 15.0, 'models', jsonb_build_object(
                   'Hermes-4-405B', jsonb_build_object('in', 0.09, 'out', 0.37),
                   'Hermes-4-70B',  jsonb_build_object('in', 0.05, 'out', 0.2))))
) on conflict (key) do nothing;

commit;
