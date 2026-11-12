"""Fixed owner-local Helheim workflow controller; no generic execution surface."""
from __future__ import annotations
import asyncio,json,shlex,subprocess
from uuid import UUID
from typing import Literal
from mcp.types import ToolAnnotations
from mcp.server.mcpserver.exceptions import ToolError

REMOTE=r'''
import json,os,stat,sys,hashlib,subprocess
from pathlib import Path
root=Path.home()/'Library/Application Support/Helheim Inference'
def private(path,directory=False):
 info=path.lstat()
 assert info.st_uid==os.getuid() and not info.st_mode&0o077
 assert (stat.S_ISDIR(info.st_mode) if directory else stat.S_ISREG(info.st_mode))
private(root,True)
manifest=root/'mcp-runtime.json';private(manifest)
assert manifest.stat().st_size<=16384
config=json.loads(manifest.read_text());assert config['schema_version']==1
python=config['python'];assert Path(python).is_absolute() and Path(python).is_file()
for name in ['control.py','live_backend.py','private_io.py']:
 path=root/name;private(path);assert path.stat().st_size<=1048576
 assert hashlib.sha256(path.read_bytes()).hexdigest()==config['files'][name]
request=json.loads(sys.stdin.buffer.read(8193));assert request['operation'] in ['status','acquire_den','release_den']
result=subprocess.run([python,'-I',str(root/'control.py')],input=json.dumps(request),text=True,capture_output=True,timeout=90)
assert len(result.stdout.encode())<=65536
reply=json.loads(result.stdout);assert type(reply.get('ok')) is bool and isinstance(reply.get('code'),str)
print(json.dumps(reply))
'''

def register(mcp,app_request,hosts):
    async def call(host,operation,request_id,expected_generation=None):
        status=await app_request(host,'status')
        if status.get('shuttingDown'):raise ToolError('App is shutting down')
        payload={'operation':operation,'request_id':str(request_id)}
        if expected_generation is not None:payload['expected_generation']=str(expected_generation)
        try:
            result=await asyncio.to_thread(subprocess.run,
                ['ssh','-o','BatchMode=yes','-o','IdentitiesOnly=yes','-o','ConnectTimeout=5',hosts[host],
                 'python3 -c '+shlex.quote(REMOTE)],input=json.dumps(payload),text=True,capture_output=True,timeout=100)
        except subprocess.TimeoutExpired:
            raise ToolError('Handoff delivery uncertain. Inspect owner status and reuse the same request ID; no automatic retry.') from None
        if result.returncode:raise ToolError('Exclusive workflow controller unavailable or code validation failed; ownership was not assumed.')
        try:reply=json.loads(result.stdout)
        except ValueError:raise ToolError('Invalid workflow controller response; inspect ownership before retrying.') from None
        if reply.get('ok') is not True:raise ToolError(json.dumps(reply))
        return reply

    @mcp.tool(annotations=ToolAnnotations(readOnlyHint=True))
    async def den_helheim_status(host:Literal['helheim']='helheim')->dict:
        """Read exclusive Helheim workflow ownership and its generation. Requires installed verified owner controller; does not start work."""
        from uuid import uuid4
        return await call(host,'status',uuid4())

    @mcp.tool(annotations=ToolAnnotations(destructiveHint=True))
    async def den_helheim_acquire(request_id:UUID,expected_generation:UUID,host:Literal['helheim']='helheim')->dict:
        """Reserve Helheim for Den after closing Hermione ingress and draining whole workflows. Current generation required. Busy/uncertain is not ownership. Reuse request_id after uncertain delivery; never steal by TTL. Start the coding worker separately only after acquired."""
        return await call(host,'acquire_den',request_id,expected_generation)

    @mcp.tool(annotations=ToolAnnotations(destructiveHint=True))
    async def den_helheim_release(request_id:UUID,expected_generation:UUID,host:Literal['helheim']='helheim')->dict:
        """Stop/drain Den and return exclusive ownership to Hermione. Can cooperatively cancel active Den work: normally wait for task completion first. Controller independently verifies native drain proof, generation fence, hub leases and inference idle. Fault never grants Hermione automatically."""
        return await call(host,'release_den',request_id,expected_generation)
