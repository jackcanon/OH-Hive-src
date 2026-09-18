#!/usr/bin/env python3
"""Run on a paired secondary. Read-only by default; --send submits ONE benign DM.
Never prints credentials or runs a worker: the desktop app must produce the reply.
"""
import argparse, ipaddress, json, uuid, urllib.request, urllib.parse
from pathlib import Path

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None

parser = argparse.ArgumentParser()
parser.add_argument('--send', action='store_true')
parser.add_argument('--after', type=int)
args = parser.parse_args()
selection = json.loads((Path.home() / 'Library/Application Support/ohhive/private-primary.json').read_text())
url = urllib.parse.urlsplit(selection['endpoint'])
assert url.scheme == 'http' and ipaddress.ip_address(url.hostname).is_private
assert not url.username and not url.password and not url.query and not url.fragment
session = str(uuid.uuid4())
client = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
def rpc(method, params=None):
    data = json.dumps(dict(session=session, method=method, params=params or {})).encode()
    req = urllib.request.Request(selection['endpoint'].rstrip('/') + '/local/v1/rpc', data=data,
        headers={'Content-Type':'application/json','Authorization':'Bearer '+selection['credential']})
    with client.open(req, timeout=10) as response:
        return json.loads(response.read(2*1024*1024))

rpc('private_fleet_identity')
actor = {'kind':'user','id':selection['owner_id']}
agents = rpc('bots_agents_list')
agent = next(a for a in agents if not a['archived'] and a['runtime_kind']=='local' and a['preferred_host']==selection['node_id'])
conversations = rpc('bots_conversations_list',{'actor':actor})
conversation = next(c for c in conversations if c['kind']=='agent_dm' and c['coordinator']==agent['id'])
print('Primary reachable; authenticated desktop-host agent:',agent['name'])
print('Profile endpoint available:', isinstance(rpc('bots_user_profile_get'),dict))
if args.send:
    message = rpc('bots_message_send',dict(actor=actor,conversation_id=conversation['id'],client_request_id='demo-check-'+str(uuid.uuid4()),
        expected_policy_revision=conversation['policy_revision'],recipient_ids=[agent['id']],
        draft=dict(thread_root=None,kind='text',body="Loki's Den connection check: please greet me naturally and state your exact configured model ID in one sentence. You are a software agent, not the physical computer.",attachment_refs=[],task_ref=None,turn_ref=None,source_event_ref=None)))
    print('Submitted one test message; sequence:',message['server_sequence'])
if args.after is not None:
    messages=rpc('bots_messages_list',dict(actor=actor,conversation_id=conversation['id'],page=dict(before=None,after=args.after,limit=20)))
    replies=[m for m in messages if m['author']=={'kind':'agent','id':agent['id']}]
    for m in replies: print('Desktop-host reply:',(m.get('body') or '')[:1000])
    if not replies: print('No new reply yet.')
print('Latest delivery states:', [r[2] for r in rpc('bots_conversation_deliveries',{'conversation':conversation['id']})[:3]])
