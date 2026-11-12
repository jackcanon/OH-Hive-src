import unittest
from unittest.mock import AsyncMock, patch
from mcp.server.mcpserver.exceptions import ToolError
import server

class BackendErrorTests(unittest.IsolatedAsyncioTestCase):
    async def test_owner_guard_reason_reaches_mcp_caller(self):
        reason='App rejected command: Stop the coding worker before changing backend.'
        with patch.object(server,'request',new=AsyncMock(side_effect=RuntimeError(reason))):
            with self.assertRaisesRegex(ToolError,'Stop the coding worker'):
                await server.den_backend_select('legacy')
