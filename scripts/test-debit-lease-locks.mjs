// PostgreSQL semantic fixture. PGlite serializes sessions: this is NOT a contention test.
// PGLITE_MODULE=/path/to/@electric-sql/pglite/dist/index.js node scripts/test-debit-lease-locks.mjs
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const path=process.env.PGLITE_MODULE;
const {PGlite}=await import(path||'@electric-sql/pglite');
const {pgcrypto}=await import(path?path.replace(/index\.js$/,'contrib/pgcrypto.js'):'@electric-sql/pglite/contrib/pgcrypto');
const db=new PGlite({extensions:{pgcrypto}});
const id=n=>`${String(n).padStart(8,'0')}-1111-1111-1111-111111111111`;
const [owner,foreign,wallet,fund,treasury,project,node,card,card2]=Array.from({length:9},(_,i)=>id(i+1));
await db.exec(`create schema hive; create schema auth; create schema extensions; create extension pgcrypto with schema extensions;
create type hive.rate_kind as enum('compute_output','compute_input');
create type hive.entry_type as enum('fund_project','spend_job','earn_compute','adjustment');
create table hive.accounts(id uuid primary key,kind text,member_id uuid);
create table hive.projects(id uuid primary key,fund_account_id uuid references hive.accounts(id),owner_id uuid,title text,deleted_at timestamptz);
create table hive.nodes(id uuid primary key,member_id uuid,display_name text,region text);
create table hive.cards(id uuid primary key,project_id uuid,key text,title text,status text,modality text);
create table hive.leases(card_id uuid primary key,node_id uuid,expires_at timestamptz,resume_from text);
create table hive.rate_table(id uuid,kind hive.rate_kind,honey_per_unit numeric,effective_from timestamptz,effective_to timestamptz);
create table hive.card_outputs(card_id uuid,node_id uuid,content text,model_id text,usage jsonb);
create table hive.notification_events(event_type text,project_id uuid,card_id uuid,member_id uuid,payload jsonb);
create table hive.checkpoint_blobs(hash text primary key,state jsonb,bytes int);
create table hive.checkpoints(card_id uuid,node_id uuid,step int,blob_hash text,usage jsonb);
create table hive.ledger_entries(txn_id uuid,account_id uuid references hive.accounts(id),entry_type hive.entry_type,direction text,amount_honey numeric check(amount_honey>=0),rate_id uuid,tokens_in bigint,tokens_out bigint,compute_seconds numeric,card_id uuid,node_id uuid,memo text,source text,anonymous boolean);
create function auth.uid() returns uuid language sql as $$select '${owner}'::uuid$$;
create function hive.is_member() returns boolean language sql as $$select true$$;
create function hive.verify_node_key(raw_key text) returns uuid language sql as $$select case when raw_key='node' then '${node}'::uuid when raw_key='foreign' then '${foreign}'::uuid else null end$$;
create function hive.ctl_delegate_node(raw_key text) returns uuid language sql as $$select hive.verify_node_key(raw_key)$$;
create function hive.account_balance(p_account uuid) returns numeric language sql stable as $$select coalesce(sum(case when direction='credit' then amount_honey else -amount_honey end),0) from hive.ledger_entries where account_id=p_account$$;
insert into hive.accounts values('${wallet}','member_wallet','${owner}'),('${fund}','project_fund',null),('${treasury}','treasury',null);
insert into hive.projects values('${project}','${fund}','${owner}','Fixture',null);
insert into hive.nodes values('${node}','${owner}','Worker','test');
insert into hive.cards values('${card}','${project}','one','One','running','text'),('${card2}','${project}','two','Two','running','text');
insert into hive.leases values('${card}','${node}',now()+interval '15 minutes',null),('${card2}','${node}',now()+interval '15 minutes',null);
insert into hive.rate_table values(gen_random_uuid(),'compute_output',1,now(),null),(gen_random_uuid(),'compute_input',0,now(),null);
insert into hive.ledger_entries(account_id,direction,amount_honey,source) values('${wallet}','credit',100,'earned'),('${fund}','credit',100,'earned');`);
async function fn(file,name){const s=await readFile(new URL('../supabase/migrations/'+file,import.meta.url),'utf8');const start=s.toLowerCase().indexOf('create or replace function hive.'+name+'(');assert(start>=0);const m=s.slice(start).match(/\bas\s+(\$\w*\$)/i);const end=s.indexOf(m[1],start+m.index+m[0].length)+m[1].length;return s.slice(start,s.indexOf(';',end)+1);}
await db.exec(await fn('20260905000013_honey_sources.sql','account_sources'));
await db.exec(await fn('20260905000004_dispatch_and_ledger.sql','current_rate'));
await db.exec(await fn('20260905000015_release_card.sql','node_release_card'));
await db.exec(await readFile(new URL('../supabase/migrations/20260915180000_debit_and_lease_locks.sql',import.meta.url),'utf8'));
const balance=async a=>Number((await db.query('select hive.account_balance($1) as b',[a])).rows[0].b);
await db.query('select hive.fund_project($1,20,false)',[project]);assert.equal(await balance(wallet),80);
await assert.rejects(()=>db.query('select hive.fund_project($1,100,false)',[project]),/insufficient_honey/);assert.equal(await balance(wallet),80);
const split=async()=> (await db.query("select hive.split_debit($1,50,array['earned'],jsonb_build_object('entry_type','fund_project')) as entries",[wallet])).rows[0].entries;
const first=await split(),stale=await split();
const post=debits=>db.query('select hive.post_txn($1::jsonb)',[JSON.stringify([...debits,{account_id:fund,entry_type:'fund_project',direction:'credit',amount:50,source:'earned'}])]);
await post(first);assert.equal(await balance(wallet),30);const before=await balance(fund);
await assert.rejects(()=>post(stale),/insufficient_honey_in_sources/);assert.equal(await balance(wallet),30);assert.equal(await balance(fund),before);
await assert.rejects(()=>split(),/insufficient_honey/);
await db.query("select hive.node_checkpoint('node',$1,1,'{}','{}')",[card]);
const complete=(name,c)=>db.query(`select hive.${name}('node',$1,'done','model',0,10,1)`,[c]);
await complete('node_complete_card',card);assert.equal(await balance(wallet),40);
await assert.rejects(()=>complete('node_complete_card',card),/no_lease/);
assert.equal((await db.query('select * from hive.card_outputs')).rows.length,1);
await assert.rejects(()=>db.query("select hive.node_checkpoint('node',$1,2,'{}','{}')",[card]),/no_lease/);
await assert.rejects(()=>db.query("select hive.ctl_d_node_fail_card('foreign',$1,'bad')",[card2]),/no_owned_lease/);
await db.query("select hive.ctl_d_node_checkpoint('node',$1,1,'{}','{}')",[card2]);
await complete('ctl_d_node_complete_card',card2);assert.equal(await balance(wallet),50);
await assert.rejects(()=>complete('ctl_d_node_complete_card',card2),/no_lease/);
await db.query("update hive.cards set status='running' where id=$1;",[card2]);await db.query('insert into hive.leases(card_id,node_id) values($1,$2)',[card2,node]);
await db.query("select hive.node_release_card('node',$1)",[card2]);
await assert.rejects(()=>complete('node_complete_card',card2),/no_lease/);
// Positive total balance must not hide overspending one source bucket.
await db.query("insert into hive.ledger_entries(account_id,direction,amount_honey,source) values($1,'credit',100,'purchased')",[wallet]);
const bucketBefore=await balance(wallet);
await assert.rejects(()=>db.query('select hive.post_txn($1::jsonb)',[JSON.stringify([
 {account_id:wallet,entry_type:'fund_project',direction:'debit',amount:60,source:'earned'},
 {account_id:fund,entry_type:'fund_project',direction:'credit',amount:60,source:'earned'}
])]),/insufficient_honey_in_sources/);
assert.equal(await balance(wallet),bucketBefore);
// A fixed Repeatable Read snapshot cannot safely recheck a ledger after waiting on a lock.
await db.exec('begin isolation level repeatable read');
await assert.rejects(()=>split(),/ledger_requires_read_committed_or_serializable/);
await db.exec('rollback');
const defs=(await db.query("select proname,provolatile,pg_get_functiondef(oid) as def from pg_proc where pronamespace='hive'::regnamespace")).rows;
for(const name of ['split_debit','post_txn','fund_project','node_complete_card','ctl_d_node_complete_card'])assert.match(defs.find(r=>r.proname===name).def,/for no key update/i);
assert.equal(defs.find(r=>r.proname==='split_debit').provolatile,'v');
for(const name of ['node_checkpoint','ctl_d_node_checkpoint','node_complete_card','ctl_d_node_complete_card'])assert.match(defs.find(r=>r.proname===name).def,/for update/i);
await db.close();console.log('PASS: actual migrated debit/funding/completion/checkpoint functions; stale debit rollback; duplicate completion and release fencing; lock definitions. Multi-session contention NOT tested.');
