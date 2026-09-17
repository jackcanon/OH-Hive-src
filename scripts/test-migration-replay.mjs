import {verifyNodeTargeting} from './migration-replay/node-targeting.mjs';
import {verifyFundedCompute} from './migration-replay/funded-compute.mjs';
// Fresh PostgreSQL-compatible replay. External Supabase services are test stand-ins;
// every Hive definition must come from migrations, never from a test fixture.
import {readFile,readdir,writeFile} from 'node:fs/promises';
import {verifyRecoveredRpcs} from './migration-replay/recovered-rpcs.mjs';
const path=process.env.PGLITE_MODULE;
const {PGlite}=await import(path||'@electric-sql/pglite');
const {pgcrypto}=await import(path?path.replace(/index\.js$/,'contrib/pgcrypto.js'):'@electric-sql/pglite/contrib/pgcrypto');
const db=new PGlite({extensions:{pgcrypto}});
try {
  await db.exec(await readFile(new URL('./migration-replay/platform.sql',import.meta.url),'utf8'));
  const dir=new URL('../supabase/migrations/',import.meta.url);
  const files=(await readdir(dir)).filter(x=>x.endsWith('.sql')).sort();
  for (const file of files) {
    let sql=await readFile(new URL(file,dir),'utf8');
    // PGlite has no pg_cron scheduler. Its API is represented by platform.sql.
    sql=sql.replace(/^create extension if not exists pg_cron with schema pg_catalog;$/gmi,'');
    try { await db.exec(sql); }
    catch(e) { console.error(`FAIL ${file}: ${e.message} (position ${e.position||'unknown'})`); process.exitCode=1; break; }
    console.log(`PASS ${file}`);
  }
  if (!process.exitCode) {
    await db.exec('select hive.assert_rls_everywhere(); select hive.assert_no_realtime()');
    if (process.env.REPLAY_FUNCTIONS) {
      const rows=(await db.query("select p.oid::regprocedure::text as signature, pg_get_functiondef(p.oid) as definition from pg_proc p join pg_namespace n on n.oid=p.pronamespace where (n.nspname='hive' or (n.nspname='public' and p.proname like 'hive_%')) and p.prokind='f' order by 1")).rows;
      await writeFile(process.env.REPLAY_FUNCTIONS,JSON.stringify({rows},null,2));
    }
    if (process.env.REPLAY_CATALOG) {
      const rows=(await db.query(await readFile(new URL('./migration-replay/catalog.sql',import.meta.url),'utf8'))).rows;
      await writeFile(process.env.REPLAY_CATALOG,JSON.stringify({rows},null,2));
    }
    await verifyRecoveredRpcs(db);
    await verifyFundedCompute(db);
    await verifyNodeTargeting(db);
    console.log(`PASS fresh replay: ${files.length} migrations; RLS and publication guards pass`);
  }
} finally { await db.close(); }
