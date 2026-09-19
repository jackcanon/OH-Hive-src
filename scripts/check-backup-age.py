#!/usr/bin/env python3
"""Read-only backup freshness probe. Schedule externally; nonzero means attention needed.
Uses the operator's existing authenticated Supabase CLI, never backup contents or keys.
"""
import argparse
import json
import math
import subprocess
import sys


def assess(rows, maximum_hours):
    if len(rows) != 1:
        raise ValueError('Expected one backup-status row')
    row = rows[0]
    age = row.get('age_hours')
    if age is None:
        return 2, 'CRITICAL: no pinned backup is recorded'
    age = float(age)
    if not math.isfinite(age) or age < 0:
        raise ValueError('Invalid backup age')
    if age > maximum_hours:
        return 2, f'CRITICAL: latest pinned backup is {age:.1f} hours old (limit {maximum_hours:g})'
    return 0, f'OK: latest pinned backup is {age:.1f} hours old; restorability not checked'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--project-ref', required=True)
    parser.add_argument('--max-age-hours', type=float, default=36)
    args = parser.parse_args()
    if not math.isfinite(args.max_age_hours) or args.max_age_hours <= 0:
        parser.error('--max-age-hours must be positive and finite')
    query = "SELECT extract(epoch FROM (now()-max(created_at)))/3600 AS age_hours FROM hive.artifacts WHERE kind='backup' AND pinned"
    try:
        result = subprocess.run(
            ['supabase', 'db', 'query', '--linked', '--project-ref', args.project_ref, query],
            capture_output=True, text=True, timeout=45, check=True,
        )
        payload = json.loads(result.stdout)
        code, message = assess(payload['rows'], args.max_age_hours)
    except (OSError, subprocess.SubprocessError, ValueError, KeyError, TypeError):
        # Do not echo CLI diagnostics which might contain connection details.
        code, message = 2, 'CRITICAL: backup freshness could not be verified; check CLI authentication/connectivity'
    print(message)
    return code


if __name__ == '__main__':
    sys.exit(main())
