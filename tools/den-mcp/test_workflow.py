import json
import subprocess
import unittest
from types import SimpleNamespace
from unittest.mock import patch
from uuid import UUID
from workflow import register, AcceptanceCheck, METHODS

ID=UUID('11111111-1111-4111-8111-111111111111')
OTHER=UUID('22222222-2222-4222-8222-222222222222')
class Registry:
    def __init__(self): self.tools={}
    def tool(self, **kwargs):
        def add(fn): self.tools[fn.__name__]=fn; return fn
        return add

class WorkflowTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        self.registry=Registry(); self.calls=[]; self.permitted=True
        async def app(*args):
            if not self.permitted: raise RuntimeError('disabled')
            return {'shuttingDown':False}
        register(self.registry,app,{'helheim':'owner@example.invalid'})
        def run(*args,**kwargs):
            self.calls.append(json.loads(kwargs['input']))
            return SimpleNamespace(returncode=0,stdout=json.dumps({'ok':True,'result':{'id':str(ID)}}))
        self.mock=patch('workflow.subprocess.run',side_effect=run);self.mock.start()
        self.addCleanup(self.mock.stop)

    async def test_coding_submission_requires_checks_and_keeps_stable_id(self):
        fn=self.registry.tools['den_task_create']
        common=dict(request_id=ID,project_id=ID,target_node_id=OTHER,agent_id=ID,
                    title='fixture',instructions='bounded fixture',model_id='fixture',max_turns=3)
        with self.assertRaises(Exception):await fn(**common,acceptance=[])
        self.assertEqual(self.calls,[])
        check=AcceptanceCheck(name='value',command='/usr/bin/python3',args=['-B','-c','assert True'])
        await fn(**common,acceptance=[check]);await fn(**common,acceptance=[check])
        self.assertEqual(self.calls[0],self.calls[1])
        self.assertEqual(self.calls[0]['params']['request']['request_id'],str(ID))
        self.assertEqual(self.calls[0]['params']['request']['max_verification_runs'],0)

    async def test_review_contract_is_read_only_snapshot_not_code_execution(self):
        await self.registry.tools['den_review_create'](ID,ID,OTHER,ID,OTHER,'fixture')
        submission=self.calls[0]['params']['request']
        self.assertEqual(submission['review_source_task_id'],str(OTHER))
        self.assertEqual(submission['acceptance'],[])
        self.assertEqual(submission['max_turns'],1)
        self.assertEqual(submission['max_acceptance_repairs'],0)

    async def test_opt_out_denies_before_ssh(self):
        self.permitted=False
        with self.assertRaises(RuntimeError):await self.registry.tools['den_task_stop'](ID)
        self.assertEqual(self.calls,[])

    async def test_shutting_down_denies_before_ssh(self):
        async def app(*args):return {'shuttingDown':True}
        register(self.registry,app,{'helheim':'owner@example.invalid'})
        with self.assertRaises(Exception):await self.registry.tools['den_projects']()
        self.assertEqual(self.calls,[])

    async def test_repository_binding_guard_preserves_existing_project(self):
        old={'repo_url':'https://github.com/example/existing.git','repo_ref':None}
        response={'ok':True,'result':[{'id':str(ID),'repository':old}]}
        with patch('workflow.subprocess.run',return_value=SimpleNamespace(returncode=0,stdout=json.dumps(response))) as run:
            with self.assertRaisesRegex(Exception,'binding changed'):
                await self.registry.tools['den_project_bind'](ID,'https://github.com/example/other.git')
            self.assertEqual(run.call_count,1,'Must not send a binding mutation')

    async def test_uncertain_delivery_is_not_automatically_retried(self):
        with patch('workflow.subprocess.run',side_effect=subprocess.TimeoutExpired('ssh',35)) as run:
            with self.assertRaisesRegex(Exception,'uncertain'):
                await self.registry.tools['den_task_run'](ID,OTHER)
            self.assertEqual(run.call_count,1)

    async def test_no_worker_claim_complete_or_unrestricted_rpc_tools(self):
        self.assertNotIn('private_run_claim',METHODS)
        self.assertNotIn('private_preparation_complete',METHODS)
        self.assertNotIn('den_rpc',self.registry.tools)
        self.assertEqual(len(self.registry.tools),16)

    async def test_preparation_run_retry_and_stop_preserve_explicit_ids(self):
        await self.registry.tools['den_task_prepare'](ID,OTHER)
        await self.registry.tools['den_task_run'](ID,OTHER)
        await self.registry.tools['den_task_retry'](ID,OTHER)
        await self.registry.tools['den_task_stop'](ID)
        self.assertEqual(self.calls[0]['params'],{'task':str(ID),'operation':str(OTHER)})
        self.assertEqual(self.calls[1]['params'],{'task':str(ID),'operation':str(OTHER)})
        self.assertEqual(self.calls[2]['params'],{'previous':str(ID),'next':str(OTHER)})
        self.assertEqual(self.calls[3]['params'],{'operation':str(ID)})

if __name__=='__main__':unittest.main()
