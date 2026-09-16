#!/usr/bin/env python3
"""Embed publisher-approved Desktop OAuth metadata without logging credential values."""
import json
import plistlib
import sys
from pathlib import Path


def embed(credential_path, plist_path, expected_id):
    credential = json.loads(Path(credential_path).read_text()).get('installed', {})
    if credential.get('client_id') != expected_id:
        raise ValueError('Desktop OAuth client does not match configured client ID')
    secret = credential.get('client_secret')
    if not isinstance(secret, str) or not secret.strip():
        raise ValueError('Desktop OAuth credential is incomplete')
    target = Path(plist_path)
    config = plistlib.loads(target.read_bytes())
    config['HiveGoogleOAuthClientSecret'] = secret
    target.write_bytes(plistlib.dumps(config))


if __name__ == '__main__':
    try:
        embed(*sys.argv[1:])
    except Exception:
        sys.exit('Could not embed Google Desktop credential. Check JSON type, client ID and file permissions.')
