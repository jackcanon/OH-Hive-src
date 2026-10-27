#!/usr/bin/env python3
"""Reject a launcher/engine built for a newer OS than the app promises to support."""
from pathlib import Path
import plistlib
import os
import re
import subprocess
import sys


def version(text):
    parts = tuple(int(part) for part in text.split('.'))
    return parts + (0,) * (3 - len(parts))


def check(app):
    contents = Path(app) / 'Contents'
    info = plistlib.loads((contents / 'Info.plist').read_bytes())
    declared = info['LSMinimumSystemVersion']
    binaries = [p for p in (contents / 'MacOS').iterdir() if p.is_file() and os.access(p, os.X_OK)]
    binaries += list((contents / 'Frameworks').glob('*.dylib'))
    cloudflared = contents / 'Resources/cloudflared'
    if cloudflared.exists():
        binaries.append(cloudflared)
    for binary in binaries:
        result = subprocess.run(['otool', '-arch', 'arm64', '-l', str(binary)],
                                capture_output=True, text=True, check=True)
        build_targets = re.findall(r'\bcmd LC_BUILD_VERSION\s+cmdsize \d+\s+platform (\d+)\s+minos ([\d.]+)', result.stdout)
        if any(platform != '1' for platform, _ in build_targets):
            raise ValueError(f'{binary.name}: not a macOS runtime binary')
        targets = [target for _, target in build_targets]
        targets += re.findall(r'\bcmd LC_VERSION_MIN_MACOSX\s+cmdsize \d+\s+version ([\d.]+)', result.stdout)
        if not targets:
            raise ValueError(f'{binary.name}: missing macOS deployment target')
        for target in targets:
            if version(target) > version(declared):
                raise ValueError(f'{binary.name} requires macOS {target}, but the bundle declares {declared}')
    return declared


if __name__ == '__main__':
    try:
        minimum = check(sys.argv[1])
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        print(f'FAIL: {error}', file=sys.stderr)
        sys.exit(1)
    print(f'PASS: all runtime binaries support declared macOS {minimum}')
