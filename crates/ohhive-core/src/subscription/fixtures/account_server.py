#!/usr/bin/python3
"""Protocol fixture: no network, credentials, or real authentication."""
import json
import sys

if '--version' in sys.argv:
    print('codex-cli 0.149.0')
    sys.exit(0)

def send(value):
    print(json.dumps(value), flush=True)

account = None
attempt = None
reads = 0
for line in sys.stdin:
    request = json.loads(line)
    method = request.get('method')
    if 'id' not in request:
        continue
    result = {}
    if method == 'account/login/start':
        attempt = 'fixture-attempt'
        reads = 0
        kind = request['params']['type']
        result = {'type': kind, 'loginId': attempt}
        result['verificationUrl' if kind == 'chatgptDeviceCode' else 'authUrl'] = 'https://auth.openai.com/fixture'
        if kind == 'chatgptDeviceCode':
            result['userCode'] = 'FIXTURE-CODE'
        send({'method': 'account/login/completed', 'params': {'loginId': 'stale-attempt', 'success': True}})
    elif method == 'account/read':
        if attempt:
            reads += 1
            if reads == 2:
                # Completion can precede account visibility; UI must retain the pending flow.
                send({'method': 'account/login/completed', 'params': {'loginId': attempt, 'success': True}})
            if reads >= 3:
                account = {'type': 'chatgpt', 'email': 'fixture@example.com', 'planType': 'plus'}
        result = {'account': account, 'requiresOpenaiAuth': True}
    elif method == 'account/login/cancel':
        # Simulate a completion racing cancellation; logout must remove this account.
        account = {'type': 'chatgpt', 'email': 'fixture@example.com', 'planType': 'plus'}
        result = {'status': 'canceled'}
    elif method == 'account/logout':
        account = None
        attempt = None
    send({'id': request['id'], 'result': result})
