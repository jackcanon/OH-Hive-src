#!/usr/bin/env python3
"""Exercise the actual launcher with disposable tiny engines, never real app data."""
from pathlib import Path
import plistlib
import subprocess
import tempfile

scripts = Path(__file__).resolve().parent
with tempfile.TemporaryDirectory(prefix='den-build-identity-') as directory:
    root = Path(directory)
    contents = root / "Loki's Den.app" / 'Contents'
    macos = contents / 'MacOS'
    frameworks = contents / 'Frameworks'
    macos.mkdir(parents=True)
    frameworks.mkdir()
    launcher = macos / 'Hive'
    subprocess.run(['xcrun', 'swiftc', str(scripts / 'Launcher.swift'), '-module-cache-path', str(root / 'module-cache'), '-o', str(launcher)], check=True)
    # Check-only mode must never execute the app.
    app = macos / 'Hive-bin'
    app.write_text('#!/bin/sh\nexit 99\n')
    app.chmod(0o755)
    def run(expected, core, success, diagnostic):
        info = {'CFBundleExecutable': 'Hive', 'CFBundleIdentifier': 'test.den.identity'}
        if expected is not None:
            info['OHHiveSourceCommit'] = expected
        (contents / 'Info.plist').write_bytes(plistlib.dumps(info))
        source = root / 'engine.c'
        source.write_text('void other_symbol(void) {}' if core is None else
                          f'const char *ohhive_core_source_commit(void) {{ return "{core}"; }}')
        subprocess.run(['xcrun', 'clang', '-dynamiclib', str(source), '-o', str(frameworks / 'libohhive_ffi.dylib')], check=True)
        result = subprocess.run([str(launcher), '--hive-bundle-check'], capture_output=True, text=True)
        assert (result.returncode == 0) == success, result.stderr
        assert diagnostic in result.stdout + result.stderr, result.stderr
    run('abc123', 'abc123', True, 'app=abc123 core=abc123')
    run('abc123', 'old456', False, 'App/core build mismatch')
    run('abc123', None, False, 'engine has no build identity')
    run(None, 'abc123', False, 'app build identity is missing')
    run('unstamped', 'unstamped', False, 'app build identity is missing')
print('PASS: matching, mismatched, missing core stamp, missing app stamp, unstamped builds')
