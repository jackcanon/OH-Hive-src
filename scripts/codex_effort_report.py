#!/usr/bin/env python3
"""Read only native token_count metadata; never emit prompt/tool/credential content.

Report cumulative-counter differences, not sums of cumulative snapshots or last_usage.
Cached input is part of input; reasoning output is part of output. No price assumptions.
"""
import argparse
from datetime import datetime
import json
from pathlib import Path

FIELDS = ('input_tokens', 'cached_input_tokens', 'cache_write_input_tokens',
          'output_tokens', 'reasoning_output_tokens', 'total_tokens')


def timestamp(value):
    return datetime.fromisoformat(value.replace('Z', '+00:00'))


def report(path, since=None, until=None):
    start = timestamp(since) if since else None
    end = timestamp(until) if until else None
    if start and end and start > end:
        raise ValueError('since must not be later than until')
    previous = None
    baseline_time = None
    last_time = None
    sums = {key: 0 for key in FIELDS}
    missing = set()
    events = intervals = resets = malformed = 0
    with Path(path).open(encoding='utf-8') as source:
        for line in source:
            try:
                row = json.loads(line)
                payload = row.get('payload') or {}
                if row.get('type') != 'event_msg' or payload.get('type') != 'token_count':
                    continue
                usage = (payload.get('info') or {}).get('total_token_usage')
                if not isinstance(usage, dict):
                    continue
                when = timestamp(row['timestamp'])
                if end and when > end:
                    continue
                values = {key: value for key, value in usage.items()
                          if key in FIELDS and type(value) is int and value >= 0}
                if 'total_tokens' not in values:
                    malformed += 1
                    continue
            except (ValueError, TypeError, KeyError, AttributeError):
                malformed += 1
                continue
            if previous and when < previous[0]:
                # Never reorder an accounting stream behind the caller's back.
                malformed += 1
                continue
            if start and when < start:
                previous = (when, values)
                continue
            events += 1
            if previous is not None:
                prev_time, prev = previous
                if baseline_time is None:
                    baseline_time = prev_time
                common = set(values) & set(prev)
                if any(values[key] < prev[key] for key in common):
                    # A reset/truncated lineage has an unknown origin. Skip that interval.
                    resets += 1
                else:
                    intervals += 1
                    for key in FIELDS:
                        if key in common:
                            sums[key] += values[key] - prev[key]
                        else:
                            missing.add(key)
            else:
                baseline_time = when
            previous = (when, values)
            last_time = when
    totals = {key: (None if key in missing or not intervals else value)
              for key, value in sums.items()}
    inp, cached = totals['input_tokens'], totals['cached_input_tokens']
    return {
        'format_version': 1,
        'source': 'Codex event_msg/token_count cumulative native counters',
        'requested_since': since, 'requested_until': until,
        'observed_baseline': baseline_time.isoformat() if baseline_time else None,
        'observed_end': last_time.isoformat() if last_time else None,
        'observed_wall_clock_seconds': ((last_time - baseline_time).total_seconds()
                                        if last_time and baseline_time else None),
        'usage_events_in_window': events, 'measured_intervals': intervals,
        'reset_intervals_excluded': resets, 'malformed_or_out_of_order_rows': malformed,
        'tokens': totals,
        'uncached_input_tokens': inp - cached if inp is not None and cached is not None and inp >= cached else None,
        'cost_usd': None,
        'notes': [
            'Counter deltas only; the first observation is a baseline, not assumed zero.',
            'A since boundary uses the last earlier counter; the first delta may straddle that boundary.',
            'Cached input and reasoning output are subsets; do not add them again to totals.',
            'Missing counters remain null; reset intervals are excluded, so coverage can be partial.',
            'Wall-clock span includes idle time; timestamps reflect usage reporting, not exact task boundaries.',
            'No dollar cost or cross-provider weighted currency is inferred from subscription usage.',
        ],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('transcript', type=Path)
    parser.add_argument('--since', help='ISO-8601 timestamp with timezone')
    parser.add_argument('--until', help='ISO-8601 timestamp with timezone')
    args = parser.parse_args()
    try:
        print(json.dumps(report(args.transcript, args.since, args.until), indent=2))
    except (OSError, ValueError, TypeError) as exc:
        parser.exit(2, f'Cannot report usage: {exc}\n')


if __name__ == '__main__':
    main()
