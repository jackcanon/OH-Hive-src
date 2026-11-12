import json, tempfile, unittest
from pathlib import Path
from uuid import uuid4
from unittest.mock import patch
from recovery import guard_other_work, save_receipt, register
from test_workflow import Registry

class GuardTests(unittest.TestCase):
    def test_rejects_unrelated_leases_and_queued_runs(self):
        for run in [{'lease_active':True,'state':'running'},{'lease_active':False,'state':'queued'}]:
            with self.assertRaisesRegex(Exception,'Unrelated'):
                guard_other_work([{'task_id':'other','run':run}],'fixture')
    def test_only_authorized_task_is_exempt_and_stopped_runs_do_not_block(self):
        guard_other_work([{'task_id':'fixture','run':{'lease_active':True,'state':'running'}}],'fixture')
        guard_other_work([{'task_id':'other','run':{'lease_active':False,'state':'queued','stop_requested':True}}],None)
    def test_receipts_are_private_atomic_and_replaceable(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'receipt.json'
            save_receipt(p,{'state':'pending'});save_receipt(p,{'state':'complete'})
            self.assertEqual(json.loads(p.read_text())['state'],'complete')
            self.assertEqual(p.stat().st_mode & 0o777,0o600)
            self.assertFalse(p.with_suffix('.tmp').exists())

class RestartTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        self.registry=Registry()
        async def app(*args):return {'shuttingDown':False}
        async def rpc(*args):return []
        register(self.registry,app,rpc,{'helheim':'owner@example.invalid'})
    async def test_unmounted_volume_stops_before_any_remote_action(self):
        with patch('recovery.os.path.ismount',return_value=False),patch('recovery.remote') as remote:
            with self.assertRaisesRegex(Exception,'not mounted'):
                await self.registry.tools['den_hub_restart'](uuid4())
            remote.assert_not_called()
    async def test_app_opt_out_stops_before_any_remote_action(self):
        async def denied(*args):raise RuntimeError('disabled')
        register(self.registry,denied,None,{'helheim':'owner@example.invalid'})
        with patch('recovery.remote') as remote:
            with self.assertRaisesRegex(Exception,'disabled'):
                await self.registry.tools['den_hub_status']()
            remote.assert_not_called()
    async def test_probe_rejects_task_not_owned_by_project(self):
        with patch('recovery.remote') as remote:
            with self.assertRaisesRegex(Exception,'not assigned'):
                await self.registry.tools['den_recovery_probe'](uuid4(),uuid4())
            remote.assert_not_called()

    async def test_primary_mismatch_refuses_before_hub_access(self):
        with patch('recovery.remote',side_effect=RuntimeError('selection changed')) as remote:
            with self.assertRaisesRegex(Exception,'selection changed'):
                await self.registry.tools['den_hub_status']()
            self.assertEqual(remote.call_count,1)
            self.assertEqual(remote.call_args.args[0],'owner@example.invalid')
