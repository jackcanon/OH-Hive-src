#!/usr/bin/env python3
"""Presence-only packaging gate; never display publisher credential values."""
import argparse
import os
from pathlib import Path
import plistlib
import sys

AVATARS = 'baldr bragi eir forseti freyja freyr frigg heimdall hel hodr idunn loki njord odin sif skadi thor tyr ullr vali vidar'.split()


def check(app, profile='publisher'):
    errors = []
    contents = Path(app) / 'Contents'
    files = ['Frameworks/libohhive_ffi.dylib', 'Resources/AppIcon.icns']
    executables = ['MacOS/Hive', 'MacOS/Hive-bin']
    if profile == 'publisher':
        executables += ['MacOS/hive-copilot-check', 'Resources/cloudflared']
    files += [f'Resources/Hive_Hive.bundle/Contents/Resources/Avatars/{name}-128.gif' for name in AVATARS]
    for relative in files + executables:
        path = contents / relative
        if not path.is_file() or path.stat().st_size == 0:
            errors.append(f'Missing or empty bundle resource: {relative}')
        elif relative in executables and not os.access(path, os.X_OK):
            errors.append(f'Bundle helper is not executable: {relative}')
    try:
        with (contents / 'Info.plist').open('rb') as stream:
            info = plistlib.load(stream)
        required = ['OHHiveSourceCommit', 'CFBundleIdentifier', 'CFBundleExecutable']
        if profile == 'publisher':
            required += ['HiveGitHubOAuthClientID', 'HiveGoogleOAuthClientID', 'HiveGoogleOAuthClientSecret']
        for key in required:
            if not isinstance(info.get(key), str) or not info[key].strip():
                errors.append(f'Missing or empty Info.plist key: {key}')
    except (OSError, ValueError, plistlib.InvalidFileException):
        errors.append('Info.plist is missing or unreadable')
    return errors


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('app', type=Path)
    parser.add_argument('--profile', choices=['publisher', 'development'], default='publisher')
    args = parser.parse_args()
    failures = check(args.app, args.profile)
    for failure in failures:
        print(f'FAIL: {failure}', file=sys.stderr)
    if not failures:
        print(f'PASS: {args.profile} bundle completeness (credential values not displayed)')
    sys.exit(bool(failures))
