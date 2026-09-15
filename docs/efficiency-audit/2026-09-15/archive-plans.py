"""Synthetic query-plan probe using the checked-out schema-6 table and quota SQL."""
import sqlite3, re, json
from pathlib import Path
root = Path('crates/ohhive-core/src/local_hub')
schema = (root/'vault_maintenance_schema.sql').read_text()
sql = re.search(r'"(SELECT coalesce\(sum\(length\(CAST\(snapshot AS BLOB\).*?)"', (root/'vault_maintenance.rs').read_text()).group(1)
db = sqlite3.connect(':memory:')
db.execute(re.search(r'CREATE TABLE vault_archives\(.*?;', schema).group())
db.executemany('INSERT INTO vault_archives VALUES(?,?,?,?,?)', [(str(i), str(i%100), 'r', 'x'*512, i) for i in range(10000)])
def plan(): return [r[3] for r in db.execute('EXPLAIN QUERY PLAN '+sql, ('42',))]
before = plan(); answer = db.execute(sql, ('42',)).fetchone()[0]
db.execute('CREATE INDEX audit_candidate_archive_vault_age ON vault_archives(vault_id,archived_ms,document_id)')
after = plan(); assert db.execute(sql, ('42',)).fetchone()[0] == answer
print(json.dumps({'sqlite_version': sqlite3.sqlite_version, 'synthetic_rows':10000,'vaults':100,'query':sql,'before':before,'candidate_index_plan':after,'same_sum_bytes':answer,'note':'Query-plan evidence only; index is in-memory, not a product migration or latency benchmark.'}, indent=2))
