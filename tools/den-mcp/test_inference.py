import json,subprocess,unittest
from types import SimpleNamespace
from unittest.mock import patch
from uuid import UUID
from mcp.server.mcpserver.exceptions import ToolError
from test_workflow import Registry
from inference import register
ID=UUID('11111111-1111-4111-8111-111111111111')
class InferenceTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        self.registry=Registry()
        async def app(*a):return {'shuttingDown':False}
        register(self.registry,app,{'helheim':'owner@example.invalid'})
    async def test_known_operation_and_expected_generation_are_preserved(self):
        with patch('inference.subprocess.run',return_value=SimpleNamespace(returncode=0,stdout=json.dumps({'ok':True,'code':'acquired','state':{'owner':'den','generation':str(ID)}}))) as run:
            reply=await self.registry.tools['den_helheim_acquire'](ID,ID)
            sent=json.loads(run.call_args.kwargs['input'])
            self.assertEqual(sent,{'operation':'acquire_den','request_id':str(ID),'expected_generation':str(ID)})
            self.assertEqual(reply['state']['generation'],str(ID))
    async def test_busy_is_an_error_not_an_assumed_reservation(self):
        with patch('inference.subprocess.run',return_value=SimpleNamespace(returncode=0,stdout=json.dumps({'ok':False,'code':'hermione_busy'}))):
            with self.assertRaisesRegex(ToolError,'hermione_busy'):
                await self.registry.tools['den_helheim_acquire'](ID,ID)
    async def test_app_optout_prevents_controller_invocation(self):
        async def denied(*a):raise RuntimeError('disabled')
        register(self.registry,denied,{'helheim':'owner@example.invalid'})
        with patch('inference.subprocess.run') as run:
            with self.assertRaisesRegex(RuntimeError,'disabled'):await self.registry.tools['den_helheim_status']()
            run.assert_not_called()
    async def test_uncertain_handoff_is_not_repeated(self):
        with patch('inference.subprocess.run',side_effect=subprocess.TimeoutExpired('ssh',100)) as run:
            with self.assertRaisesRegex(ToolError,'uncertain'):
                await self.registry.tools['den_helheim_release'](ID,ID)
            self.assertEqual(run.call_count,1)
