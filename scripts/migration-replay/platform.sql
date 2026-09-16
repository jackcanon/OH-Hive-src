-- Test-only Supabase platform stand-ins. No Hive tables/functions may be stubbed here.
create role anon; create role authenticated; create role service_role bypassrls;
create schema auth; create schema extensions; create schema storage; create schema vault; create schema cron;
create extension pgcrypto with schema extensions;
create table auth.users(id uuid primary key);
create table public.profiles(id uuid primary key references auth.users(id), display_name text, full_name text, avatar_url text, email text);
create function auth.uid() returns uuid language sql stable as $$select nullif(current_setting('request.jwt.claim.sub',true),'')::uuid$$;
create function auth.jwt() returns jsonb language sql stable as $$select coalesce(nullif(current_setting('request.jwt.claims',true),''),'{}')::jsonb$$;
create table storage.buckets(id text primary key,name text,public boolean, file_size_limit bigint,allowed_mime_types text[]);
create table storage.objects(id uuid primary key,bucket_id text,name text,owner uuid);
create function storage.foldername(text) returns text[] language sql immutable as $$select string_to_array($1,'/')$$;
create table vault.secrets(id uuid primary key default gen_random_uuid(),secret text,name text,description text);
create view vault.decrypted_secrets as select *,secret as decrypted_secret from vault.secrets;
create function vault.create_secret(text,text default null,text default null) returns uuid language sql as $$insert into vault.secrets(secret,name,description) values($1,$2,$3) returning id$$;
create table cron.job(jobid bigserial primary key,jobname text unique,schedule text,command text);
create function cron.schedule(text,text,text) returns bigint language sql as $$insert into cron.job(jobname,schedule,command) values($1,$2,$3) on conflict(jobname) do update set schedule=excluded.schedule,command=excluded.command returning jobid$$;
create function cron.unschedule(bigint) returns boolean language sql as $$with d as (delete from cron.job where jobid=$1 returning *) select exists(select from d)$$;
