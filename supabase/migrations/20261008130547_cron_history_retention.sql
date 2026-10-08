-- Completed history only: 7 days success, 30 days failures, active/unknown preserved.
CREATE OR REPLACE FUNCTION hive.cleanup_cron_history(p_limit integer DEFAULT 2000)
RETURNS integer LANGUAGE plpgsql SECURITY DEFINER SET search_path TO pg_catalog
AS $function$
DECLARE removed integer;
BEGIN
 WITH expired AS (
  SELECT runid FROM cron.job_run_details
  WHERE end_time IS NOT NULL AND
   ((status='succeeded' AND end_time < now()-interval '7 days')
     OR (status='failed' AND end_time < now()-interval '30 days'))
  ORDER BY runid LIMIT least(greatest(coalesce(p_limit,2000),1),5000)
  FOR UPDATE SKIP LOCKED
 ) DELETE FROM cron.job_run_details d USING expired e WHERE d.runid=e.runid;
 GET DIAGNOSTICS removed = ROW_COUNT;
 RETURN removed;
END $function$;
REVOKE ALL ON FUNCTION hive.cleanup_cron_history(integer) FROM PUBLIC,anon,authenticated,service_role;
-- 48,000-row/day capacity exceeds current approximately 5,800 daily arrivals.
SELECT cron.schedule('hive_cron_history_retention','11 * * * *','select hive.cleanup_cron_history(2000)');
