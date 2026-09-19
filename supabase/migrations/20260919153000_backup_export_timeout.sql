-- Backup exports cover the complete Hive schema and outgrow the normal RPC timeout.
-- PostgREST hoists this function-specific setting; other endpoints keep their limits.
-- Authorization remains in hive.backup_export: only an online HJM node may export.
-- Rollback: ALTER FUNCTION public.hive_backup_export(text) RESET statement_timeout;
ALTER FUNCTION public.hive_backup_export(text) SET statement_timeout = '30s';
NOTIFY pgrst, 'reload schema';
