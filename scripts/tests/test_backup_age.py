import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('backup_age', Path(__file__).parents[1] / 'check-backup-age.py')
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class BackupAgeTests(unittest.TestCase):
    def test_stale_backup_alerts(self):
        self.assertEqual(probe.assess([{'age_hours': '66.93'}], 36)[0], 2)

    def test_missing_backup_alerts(self):
        self.assertEqual(probe.assess([{'age_hours': None}], 36)[0], 2)

    def test_fresh_backup_passes(self):
        self.assertEqual(probe.assess([{'age_hours': 2}], 36)[0], 0)

    def test_invalid_results_cannot_pass(self):
        for rows in ([], [{'age_hours': -1}], [{'age_hours': 'NaN'}]):
            with self.assertRaises(ValueError):
                probe.assess(rows, 36)


if __name__ == '__main__':
    unittest.main()
