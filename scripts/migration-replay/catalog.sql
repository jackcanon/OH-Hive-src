select 'column' kind,c.relname object,jsonb_build_object('name',a.attname,'type',format_type(a.atttypid,a.atttypmod),'notnull',a.attnotnull,'default',pg_get_expr(d.adbin,d.adrelid),'position',a.attnum,'identity',a.attidentity) detail
from pg_class c join pg_namespace n on n.oid=c.relnamespace join pg_attribute a on a.attrelid=c.oid left join pg_attrdef d on d.adrelid=c.oid and d.adnum=a.attnum
where n.nspname='hive' and c.relkind in ('r','p') and a.attnum>0 and not a.attisdropped
union all select 'table',c.relname,jsonb_build_object('rls',c.relrowsecurity,'acl',c.relacl) from pg_class c join pg_namespace n on n.oid=c.relnamespace where n.nspname='hive' and c.relkind in ('r','p')
union all select 'constraint',c.relname,jsonb_build_object('name',con.conname,'definition',pg_get_constraintdef(con.oid)) from pg_constraint con join pg_class c on c.oid=con.conrelid join pg_namespace n on n.oid=c.relnamespace where n.nspname='hive'
union all select 'index',tablename,jsonb_build_object('name',indexname,'definition',indexdef) from pg_indexes where schemaname='hive'
union all select 'policy',tablename,jsonb_build_object('name',policyname,'command',cmd,'roles',roles,'qual',qual,'check',with_check) from pg_policies where schemaname='hive'
union all select 'trigger',c.relname,jsonb_build_object('name',t.tgname,'definition',pg_get_triggerdef(t.oid),'enabled',t.tgenabled) from pg_trigger t join pg_class c on c.oid=t.tgrelid join pg_namespace n on n.oid=c.relnamespace where n.nspname='hive' and not t.tgisinternal
union all select 'view',viewname,jsonb_build_object('definition',definition) from pg_views where schemaname='hive'
union all select 'function_acl',n.nspname||'.'||p.proname,jsonb_build_object('arguments',pg_get_function_identity_arguments(p.oid),'acl',p.proacl,'security_definer',p.prosecdef,'volatility',p.provolatile) from pg_proc p join pg_namespace n on n.oid=p.pronamespace where (n.nspname='hive' or(n.nspname='public' and p.proname like 'hive_%')) and p.prokind='f'
union all select 'enum' as kind,t.typname as object,jsonb_build_object('values',jsonb_agg(e.enumlabel order by e.enumsortorder)) as detail from pg_type t join pg_namespace n on n.oid=t.typnamespace join pg_enum e on e.enumtypid=t.oid where n.nspname='hive' group by t.typname
union all select 'sequence',sequencename,jsonb_build_object('type',data_type,'start',start_value,'min',min_value,'max',max_value,'increment',increment_by,'cycle',cycle,'cache',cache_size) from pg_sequences where schemaname='hive'
order by kind,object;
