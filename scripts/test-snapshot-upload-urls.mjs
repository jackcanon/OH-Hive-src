// Focused actual PostgreSQL function fixture, not a full production migration replay.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const {PGlite} = await import(process.env.PGLITE_MODULE || '@electric-sql/pglite');
const db = new PGlite();
const me='11111111-1111-1111-1111-111111111111', other='22222222-2222-2222-2222-222222222222';
await db.exec(`
create schema hive; create schema auth;
create function auth.uid() returns uuid language sql as $$select nullif(current_setting('test.uid',true),'')::uuid$$;
create table hive.members(id uuid, bio text, avatar_choice text, custom_avatar_url text);
insert into hive.members(id) values('${me}');
create function hive.is_member() returns boolean language sql as $$select exists(select 1 from hive.members where id=auth.uid())$$;
create table hive.bug_reports(id uuid,member_id uuid);
insert into hive.bug_reports values('${me}','${me}'),('${other}','${other}');
create table hive.bug_report_attachments(bug_report_id uuid,url text not null);
create table hive.nodes(id uuid,display_name text,region text);
insert into hive.nodes values('${me}','coordinator','test');
create function hive.verify_node_key(text) returns uuid language sql as $$select case when $1='valid' then '${me}'::uuid end$$;
create table hive.coordinator_lease(singleton boolean,node_id uuid,expires_at timestamptz);
insert into hive.coordinator_lease values(true,'${me}',now()+interval '1 hour');
create table public.profiles(id uuid,display_name text);
create table hive.projects(id uuid,title text,goal text,license_kind text,license_spdx text,requires_internet boolean,created_at timestamptz,owner_id uuid,fund_account_id uuid,deleted_at timestamptz,execution_mode text);
insert into hive.projects(id,title,goal,execution_mode) values('${me}','public','shared','hive'),('${other}','SECRET','private goal','local');
insert into hive.projects(id,title,execution_mode,deleted_at) values(gen_random_uuid(),'deleted','hive',now());
create table hive.cards(project_id uuid,status text);
insert into hive.cards values('${me}','ready'),('${other}','private_status');
create function hive.account_balance(uuid) returns numeric language sql as 'select 0::numeric';
create function hive.capacity_summary() returns jsonb language sql as $$select '{}'::jsonb$$;
create table hive.rate_table(kind text,honey_per_unit numeric,model_ref text,effective_from timestamptz,effective_to timestamptz);
create table hive.regional_servers(node_id uuid,tier text,status text,public_url text);
`);
await db.exec(await readFile(new URL('../supabase/migrations/20260915190000_snapshot_and_upload_urls.sql',import.meta.url),'utf8'));
await db.query("select set_config('test.uid',$1,false)",[me]);
const result=await db.query("select hive.snapshot_source('valid') as data");
assert.equal(result.rows[0].data.projects.length,1);
assert.equal(result.rows[0].data.projects[0].title,'public');
assert.ok(!JSON.stringify(result.rows).includes('private'));
await assert.rejects(db.query("select hive.snapshot_source('invalid')"),/invalid_or_revoked_node_key/);
await db.exec("update hive.coordinator_lease set expires_at=now()-interval '1 hour'");
await assert.rejects(db.query("select hive.snapshot_source('valid')"),/not_the_coordinator/);
const base='https://project.supabase.co/storage/v1/object/public/';
const avatar=base+'avatars/'+me+'/avatar';
const attachment=base+'bug-attachments/'+me+'/'+me+'-screenshot%20(1).png';
const profile=url=>db.query('select hive.member_update_profile(null, null, $1)',[url]);
const attach=url=>db.query('select hive.bug_report_add_attachment($1,$2)',[me,url]);
await profile(avatar); await profile(avatar+'?v=123456'); await profile(null);
await attach(attachment);
for (const [url,call,error] of [[avatar,profile,/avatar_url_not_your_own_upload/],[attachment,attach,/attachment_not_your_own_upload/]]) {
 for (const bad of ['javascript:alert(1)//'+url,'data:text/plain,'+url,'x'+url,url.replace('https:','http:'),url.replace(me,other),url.replace('project.supabase.co','user@project.supabase.co'),url+'#fragment',url+'?redirect=evil',url+'\n',url+'/../evil',url.replace('/storage/','/prefix/storage/')]) {
  await assert.rejects(call(bad),error,bad);
 }
}
await assert.rejects(attach(null),/attachment_not_your_own_upload/);
await assert.rejects(attach(attachment+'%2fmore'),/attachment_not_your_own_upload/);
await assert.rejects(attach(attachment+'%0a'),/attachment_not_your_own_upload/);
await assert.rejects(db.query('select hive.bug_report_add_attachment($1,$2)',[other,attachment]),/bug_report_not_found/);
assert.equal((await db.query('select count(*)::int as n from hive.bug_report_attachments')).rows[0].n,1);
assert.equal((await db.query('select custom_avatar_url from hive.members')).rows[0].custom_avatar_url,avatar+'?v=123456');
await db.query("select set_config('test.uid',$1,false)",[other]);
await assert.rejects(profile(avatar),/not_a_member/);
await assert.rejects(attach(attachment),/not_a_member/);
await db.close();
console.log('PASS: private snapshot filtering, coordinator guards, whole HTTPS upload URLs, ownership, nulls, preserved avatar versions, rejection without mutation');
