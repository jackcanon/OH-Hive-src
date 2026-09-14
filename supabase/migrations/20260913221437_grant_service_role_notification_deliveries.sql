-- Fix: bridge-telegram Edge Function (cron job 7, every 20s) has been hitting
-- "permission denied for table notification_deliveries" on every single run --
-- this table never got a service_role grant, unlike every other DML target that
-- table has (postgres has full DML; authenticated only has SELECT). service_role
-- is the trusted key Edge Functions authenticate with, so this is a plain missing
-- grant, not an RLS/security decision -- delivery-tracking rows carry no
-- cross-member exposure concern service_role doesn't already have via the API key.
grant select, insert, update, delete on hive.notification_deliveries to service_role;
