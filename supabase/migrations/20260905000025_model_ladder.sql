-- OH Hive — model ladder for first-run setup (ADR-010). Safe to re-run.
--
-- The desktop app assesses a machine's accelerator memory and picks the most capable model that
-- fits. The ladder lives here so the recommendation can improve without shipping a new app;
-- the app carries the same list as a fallback. Best rung first. Sizes in bytes.

insert into hive.settings (key, value) values ('model_ladder', jsonb_build_array(
  jsonb_build_object('model', 'qwen3.6:27b',        'min_bytes', 22::bigint * 1073741824, 'download_bytes', 17::bigint * 1073741824, 'why', 'strongest general model the Hive uses; needs ~22 GB'),
  jsonb_build_object('model', 'gemma4:12b-it-qat',  'min_bytes', 11::bigint * 1073741824, 'download_bytes',  8::bigint * 1073741824, 'why', 'the Hive''s interview model — fast, capable, fits in 12 GB'),
  jsonb_build_object('model', 'gemma3:4b',          'min_bytes',  5::bigint * 1073741824, 'download_bytes', 3543348019,               'why', 'small but real; good for short text cards'),
  jsonb_build_object('model', 'gemma3:1b',          'min_bytes',  2::bigint * 1073741824, 'download_bytes',  1::bigint * 1073741824, 'why', 'tiny — keeps a low-memory machine useful for simple cards')
)) on conflict (key) do nothing;

create or replace function hive.model_ladder() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select coalesce((select value from hive.settings where key = 'model_ladder'), '[]'::jsonb);
$$;
create or replace function public.hive_model_ladder() returns jsonb
language sql stable security definer set search_path = hive, public as $$ select hive.model_ladder(); $$;
grant execute on function public.hive_model_ladder() to anon, authenticated;
