#!/usr/bin/env python3
"""Test actual Mach-O deployment targets, independently of the current host version."""
from pathlib import Path
import plistlib
import subprocess
import tempfile

scripts = Path(__file__).resolve().parent
with tempfile.TemporaryDirectory(prefix='den-macos-target-') as directory:
    root = Path(directory)
    app = root / 'Den.app'
    contents = app / 'Contents'
    (contents / 'MacOS').mkdir(parents=True)
    (contents / 'Frameworks').mkdir()
    (contents / 'Info.plist').write_bytes(plistlib.dumps({'LSMinimumSystemVersion': '27.0'}))
    source = root / 'main.c'
    source.write_text('int main(void) { return 0; }')
    def build(name, minimum):
        subprocess.run(['xcrun', 'clang', '-arch', 'arm64', '-mmacosx-version-min='+minimum,
                        str(source), '-o', str(contents / 'MacOS' / name)], check=True,
                       capture_output=True)
    def verify(success):
        result = subprocess.run(['python3', str(scripts / 'check-macos-target.py'), str(app)],
                                capture_output=True, text=True)
        assert (result.returncode == 0) == success, result.stdout + result.stderr
        return result.stdout + result.stderr
    build('Hive', '27.0')
    build('Hive-bin', '27.0')
    verify(True)
    build('Hive', '27.2')
    assert 'Hive requires macOS 27.2' in verify(False)
    build('Hive', '27.0')
    build('Hive-bin', '27.2')
    assert 'Hive-bin requires macOS 27.2' in verify(False)
print('PASS: compatible runtime and incompatible launcher/app deployment targets')
