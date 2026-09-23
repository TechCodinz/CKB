import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch
import stage_ckb


class CkbOnlyTests(unittest.TestCase):
    def test_internal_ports_and_persistent_volume(self):
        config = stage_ckb.runtime_config(Path('/source'),{'KEY':'a$b'}, {})
        self.assertEqual(set(config['services']),{'mcp','registry'})
        self.assertEqual(config['services']['mcp']['ports'],['127.0.0.1:18083:10000'])
        self.assertEqual(config['services']['registry']['ports'],['127.0.0.1:18084:10000'])
        self.assertEqual(config['services']['mcp']['environment']['KEY'],'a$$b')
        self.assertIn('ckb_reality_data:/app/ckb_reality_data',config['services']['mcp']['volumes'])

    def test_ckb_flow_never_requests_vercel(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.ExitStack() as stack:
            s = stage_ckb.shared
            stack.enter_context(patch.object(s,'BASE',Path(directory)))
            stack.enter_context(patch.object(stage_ckb.os,'geteuid',return_value=0))
            stack.enter_context(patch.object(stage_ckb.socket,'socket'))
            stack.enter_context(patch.object(stage_ckb.shutil,'disk_usage',return_value=Mock(free=20*1024**3)))
            prompt = stack.enter_context(patch.object(s.getpass,'getpass',return_value='render-token'))
            stack.enter_context(patch.object(s,'render_values',side_effect=[{}, {'CKB_API_KEY':'registry-key'}]))
            vercel = stack.enter_context(patch.object(s,'vercel_values',side_effect=AssertionError('Unexpected Vercel request')))
            stack.enter_context(patch.object(s,'command',return_value=json.dumps({'services':{'api':{'environment':{'INTERNAL_API_SECRET':'original-key'}}}})))
            stack.enter_context(patch.object(s,'source'))
            deploy = stack.enter_context(patch.object(s,'stage'))
            response = stack.enter_context(patch.object(s,'urlopen'))
            response.return_value.__enter__.return_value.status = 200
            stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
            stage_ckb.main()
            prompt.assert_called_once_with('Render API key (hidden): ')
            vercel.assert_not_called()
            self.assertEqual(deploy.call_args.args[0],'ckb-runtime')
            self.assertEqual(deploy.call_args.args[4]['mcp']['CKB_INTERNAL_SECRET'],'original-key')


if __name__ == '__main__':
    unittest.main()
