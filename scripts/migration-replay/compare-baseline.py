#!/usr/bin/env python3
"""Compare replayed schema with the approved schema-only production reference.
Whitespace/comment normalization preserves quoted SQL string contents. This is a
change detector, not proof that arbitrary SQL expressions are semantically equivalent.
"""
import argparse, hashlib, json, re
from pathlib import Path

def body_hash(definition):
    match = re.search(r'AS (\$\w*\$)([\s\S]*)\1', definition)
    body = match.group(2) if match else definition
    tokens = [m.group() if m.group().startswith(("'", '"')) else m.group().lower()
              for m in re.finditer(r"'(?:''|[^'])*'|\"(?:\"\"|[^\"])*\"|--[^\n]*|/\*[\s\S]*?\*/|[A-Za-z_][A-Za-z_0-9]*|\d+(?:\.\d+)?|[^\s]", body)
              if not m.group().startswith(('--', '/*'))]
    return hashlib.sha256(json.dumps(tokens).encode()).hexdigest()

def object_key(row):
    return (row['kind'], row['object'], row['detail'].get('name', row['detail'].get('arguments', '')))

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--catalog', required=True)
    parser.add_argument('--functions', required=True)
    args = parser.parse_args()
    baseline = Path(__file__).resolve().parents[2] / 'docs/schema-baseline/2026-09-16'
    live = json.loads((baseline / 'catalog.json').read_text())['rows']
    local = json.loads(Path(args.catalog).read_text())['rows']
    current = {object_key(row): row for row in local}
    missing = [object_key(row) for row in live if object_key(row) not in current]
    if missing:
        raise SystemExit(f'Missing baseline objects: {missing}')
    # No pending change intentionally rewrites any existing table column or constraint.
    for row in live:
        if row['kind'] in ('column', 'constraint', 'sequence'):
            old = {k:v for k,v in row['detail'].items() if k != 'position'}
            new = {k:v for k,v in current[object_key(row)]['detail'].items() if k != 'position'}
            if old != new:
                raise SystemExit(f'Unexpected structural drift: {object_key(row)}')
    for row in live:
        if row['kind'] == 'enum':
            actual_values = current[object_key(row)]['detail']['values']
            if [v for v in actual_values if v in row['detail']['values']] != row['detail']['values']:
                raise SystemExit(f'Enum values removed or reordered: {row["object"]}')
    manifest = json.loads((baseline / 'function-manifest.json').read_text())
    actual = {row['signature']: body_hash(row['definition']) for row in json.loads(Path(args.functions).read_text())['rows']}
    changed = {name for name, digest in manifest['body_hashes'].items() if actual.get(name) != digest}
    allowed = set(manifest['intentional_changes'])
    if changed != allowed:
        raise SystemExit(f'Unreviewed function drift: {sorted(changed-allowed)}; missing expected corrections: {sorted(allowed-changed)}')
    print(f"PASS schema baseline: all {len(live)} catalog objects represented; {len(manifest['body_hashes'])} functions accounted for, {len(changed)} documented pending body changes")

if __name__ == '__main__':
    main()
