import json
from pathlib import Path
import tempfile
import unittest
from codex_effort_report import report


def row(second, total, cached=0):
    return {'timestamp': f'2026-09-15T12:00:{second:02}Z', 'type': 'event_msg',
            'payload': {'type': 'token_count', 'info': {'total_token_usage':
            {'input_tokens': total, 'cached_input_tokens': cached,
             'output_tokens': 0, 'total_tokens': total}}}}


class EffortTests(unittest.TestCase):
    def run_report(self, rows, **kwargs):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'usage.jsonl'
            path.write_text('\n'.join(json.dumps(r) for r in rows))
            return report(path, **kwargs)

    def test_cumulative_duplicates_not_double_counted(self):
        result = self.run_report([row(0, 100, 80), row(1, 200, 150), row(2, 200, 150), row(3, 260, 180)])
        self.assertEqual(result['tokens']['total_tokens'], 160)
        self.assertEqual(result['uncached_input_tokens'], 60)
        self.assertIsNone(result['tokens']['reasoning_output_tokens'])

    def test_since_uses_prior_counter_not_whole_session_total(self):
        result = self.run_report([row(0, 1000), row(5, 1100), row(9, 1200)],
                                 since='2026-09-15T12:00:04Z', until='2026-09-15T12:00:06Z')
        self.assertEqual(result['tokens']['total_tokens'], 100)
        self.assertEqual(result['observed_wall_clock_seconds'], 5)

    def test_reset_excludes_unknown_boundary(self):
        result = self.run_report([row(0, 100), row(1, 200), row(2, 10), row(3, 30)])
        self.assertEqual(result['tokens']['total_tokens'], 120)
        self.assertEqual(result['reset_intervals_excluded'], 1)

    def test_no_measurement_is_not_zero_and_content_not_exposed(self):
        result = self.run_report([{'type': 'response_item', 'payload': {'text': 'SECRET'}}, row(1, 20)])
        self.assertIsNone(result['tokens']['total_tokens'])
        self.assertNotIn('SECRET', json.dumps(result))


if __name__ == '__main__':
    unittest.main()
