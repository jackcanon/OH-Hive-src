#!/usr/bin/env python3
"""Test omissions against synthetic bundles; no credentials or workers involved."""
import importlib.util
from pathlib import Path
import plistlib
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('gate', Path(__file__).with_name('check-bundle-completeness.py'))
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)

class CompletenessTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.app = Path(self.temp.name) / "Loki's Den.app"
        self.contents = self.app / 'Contents'
        self.resources = ['Frameworks/libohhive_ffi.dylib', 'Resources/AppIcon.icns',
                          'MacOS/Hive', 'MacOS/Hive-bin', 'MacOS/hive-copilot-check', 'Resources/cloudflared']
        self.resources += [f'Resources/Hive_Hive.bundle/Contents/Resources/Avatars/{name}-128.gif' for name in gate.AVATARS]
        for relative in self.resources:
            path = self.contents / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'fixture')
            path.chmod(0o755)
        self.info = {key: 'synthetic-value-never-print' for key in ['OHHiveSourceCommit', 'CFBundleIdentifier', 'CFBundleExecutable', 'HiveGitHubOAuthClientID', 'HiveGoogleOAuthClientID', 'HiveGoogleOAuthClientSecret']}
        self.write_info()

    def write_info(self):
        (self.contents / 'Info.plist').write_bytes(plistlib.dumps(self.info))

    def test_complete(self):
        self.assertEqual(gate.check(self.app), [])

    def test_every_resource_required(self):
        for relative in self.resources:
            with self.subTest(resource=relative):
                path = self.contents / relative
                path.unlink()
                self.assertTrue(gate.check(self.app))
                path.write_bytes(b'fixture')
                path.chmod(0o755)

    def test_every_key_required_and_no_values_in_errors(self):
        for key in list(self.info):
            with self.subTest(key=key):
                value = self.info.pop(key)
                self.write_info()
                failures = gate.check(self.app)
                self.assertTrue(failures)
                self.assertNotIn(value, '\n'.join(failures))
                self.info[key] = value
        self.write_info()

    def test_empty_secret_and_nonexecutable_helper(self):
        self.info['HiveGoogleOAuthClientSecret'] = ' '
        self.write_info()
        (self.contents / 'Resources/cloudflared').chmod(0o644)
        self.assertEqual(len(gate.check(self.app)), 2)

    def test_explicit_development_profile(self):
        for key in ['HiveGitHubOAuthClientID', 'HiveGoogleOAuthClientID', 'HiveGoogleOAuthClientSecret']:
            self.info.pop(key)
        self.write_info()
        for relative in ['MacOS/hive-copilot-check', 'Resources/cloudflared']:
            (self.contents / relative).unlink()
        self.assertTrue(gate.check(self.app))
        self.assertEqual(gate.check(self.app, 'development'), [])

    def test_invalid_plist(self):
        (self.contents / 'Info.plist').write_bytes(b'bad plist')
        self.assertTrue(gate.check(self.app))

if __name__ == '__main__':
    unittest.main()
