// Local, in-memory PostgreSQL restore verification. Never connects to production.
// Input is sensitive: report only table names/counts and SQLSTATE, never row/error detail.
import {readFile,readdir} from 'node:fs/promises';
import {spawnSync} from 'node:child_process';
import {PGlite} from './migration-replay/node_modules/@electric-sql/pglite/dist/index.js';
import {pgcrypto} from './migration-replay/node_modules/@electric-sql/pglite/dist/contrib/pgcrypto.js';
const input=process.argv[2];
if(!input) throw new Error('Usage: node scripts/test-backup-restore.mjs /private/path/backup.json');
const doc=JSON.parse(await readFile(input,'utf8'));
const db=new PGlite({extensions:{pgcrypto}});
let phase='platform';
try {
 await db.exec(await readFile(new URL('./migration-replay/platform.sql',import.meta.url),'utf8'));
 const dir=new URL('../supabase/migrations/',import.meta.url);
 for(const file of (await readdir(dir)).filter(x=>x.endsWith('.sql')).sort()) {
  phase=`migration ${file}`;
  const sql=(await readFile(new URL(file,dir),'utf8')).replace(/^create extension if not exists pg_cron with schema pg_catalog;$/gmi,'');
  await db.exec(sql);
 }
 console.log('PASS fresh schema replay');
 // Migrations seed accounts with new random IDs. A full recovery replaces those
 // fixture rows, rather than silently skipping backup IDs on unique-key conflicts.
 const hiveTables=(await db.query("SELECT tablename FROM pg_tables WHERE schemaname='hive'")).rows;
 await db.exec('TRUNCATE '+hiveTables.map(({tablename})=>'hive."'+tablename.replaceAll('"','""')+'"').join(',')+' CASCADE');
 console.log('Prepared empty Hive tables in disposable in-memory database');
 if(process.argv.includes('--auth-placeholders')) {
  // Test-only prerequisites, NOT recovered accounts or authentication data.
  const refs=(await db.query(`SELECT c.relname AS tbl,a.attname AS col
   FROM pg_constraint f JOIN pg_class c ON c.oid=f.conrelid
   JOIN pg_namespace n ON n.oid=c.relnamespace
   JOIN pg_attribute a ON a.attrelid=c.oid AND a.attnum=f.conkey[1]
   WHERE f.contype='f' AND n.nspname='hive' AND f.confrelid IN ('auth.users'::regclass,'public.profiles'::regclass)`)).rows;
  for(const {tbl,col} of refs) for(const row of doc.tables[tbl]||[]) if(row[col]) {
   await db.query('INSERT INTO auth.users(id) VALUES ($1) ON CONFLICT DO NOTHING',[row[col]]);
   await db.query('INSERT INTO public.profiles(id) VALUES ($1) ON CONFLICT DO NOTHING',[row[col]]);
  }
  console.log('LIMITATION: synthetic auth ID prerequisites; real auth data is not backed up');
 }
 phase='restore SQL generation';
 const generated=spawnSync('python3',[new URL('./backup-to-sql.py',import.meta.url).pathname,input],{encoding:'utf8',maxBuffer:128*1024*1024});
 if(generated.status!==0) throw new Error('Generator failed');
 phase='restore transaction';
 await db.exec('BEGIN');
 await db.exec(generated.stdout);
 await db.exec('COMMIT');
 console.log('PASS restore transaction');
 await db.exec('BEGIN');
 await db.exec(generated.stdout);
 await db.exec('COMMIT');
 console.log('PASS second restore (idempotency)');
 const seqs=(await db.query(`SELECT table_name,column_name FROM information_schema.columns
 WHERE table_schema='hive' AND (is_identity='YES' OR column_default LIKE 'nextval(%')`)).rows;
 for(const {table_name,column_name} of seqs) {
  const seq=(await db.query('SELECT pg_get_serial_sequence($1,$2) AS name',['hive.'+table_name,column_name])).rows[0].name;
  if(seq) {
   const {rows}=await db.query(`SELECT nextval($1::regclass) > coalesce((SELECT max("${column_name}") FROM hive."${table_name}"),0) AS valid`,[seq]);
   if(!rows[0].valid) throw new Error('Sequence not advanced');
  }
 }
 console.log('PASS next sequence values exceed restored IDs');
 for(const table of doc.order) {
  phase=`verify ${table}`;
  if(!/^[a-z_][a-z0-9_]*$/.test(table)) throw new Error('Invalid table name');
  const expected=doc.tables[table]||[];
  // Exact record comparison catches dropped rows and ON CONFLICT skipping different data.
  const q=`SELECT count(*)::int AS missing FROM (SELECT * FROM jsonb_populate_recordset(NULL::hive."${table}", $1::jsonb) EXCEPT SELECT * FROM hive."${table}") x`;
  const {rows}=await db.query(q,[JSON.stringify(expected)]);
  const count=(await db.query(`SELECT count(*)::int AS n FROM hive."${table}"`)).rows[0].n;
  if(rows[0].missing!==0 || count!==expected.length) throw new Error('Restored rows differ');
 }
 console.log(`PASS exact record verification: ${doc.order.length} tables`);
} catch(e) {
 console.error(`FAIL ${phase}; SQLSTATE ${e.code||'unavailable'}; table ${e.table||'unavailable'}; column ${e.column||'unavailable'}; constraint ${e.constraint||'unavailable'} (details withheld to protect backup contents)`);
 process.exitCode=1;
} finally {await db.close();}
