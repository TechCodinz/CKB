import unittest
from unittest.mock import Mock, patch
import stage

class ImportTests(unittest.TestCase):
    def test_only_decrypted_production_values(self):
        values = stage.production_values([
            {'key':'JWT_SECRET','value':'original$secret','target':['production'], 'decrypted':True},
            {'key':'JWT_SECRET','value':'preview','target':['preview'], 'decrypted':True},
        ])
        self.assertEqual(values, {'JWT_SECRET':'original$secret'})

    def test_unavailable_production_secret_stops(self):
        with self.assertRaises(stage.Stop):
            stage.production_values([{'key':'ENCRYPTION_KEY','target':['production'],'type':'sensitive'}])

    def test_encrypted_ciphertext_not_accepted(self):
        with self.assertRaises(stage.Stop):
            stage.production_values([{'key':'JWT_SECRET','value':'ciphertext','target':['production'],'type':'encrypted','decrypted':False}])

    def test_duplicate_production_key_stops(self):
        with self.assertRaises(stage.Stop):
            stage.production_values([{'key':'X','value':v,'target':['production'],'decrypted':True} for v in ('a','b')])

    def test_preserve_dollars_in_compose(self):
        self.assertEqual(stage.compose_env({'KEY':'a${B}$c'}), {'KEY':'a$${B}$$c'})

    def test_encrypted_list_value_resolved_by_id(self):
        fetch = Mock(return_value={'id':'env1','key':'JWT_SECRET','type':'encrypted',
                                  'decrypted':True,'value':'original$secret'})
        rows = [{'id':'env1','key':'JWT_SECRET','target':['production'],
                 'type':'encrypted','value':'ciphertext'}]
        self.assertEqual(stage.production_values(rows, fetch), {'JWT_SECRET':'original$secret'})
        fetch.assert_called_once_with('env1')

    def test_sensitive_never_fetched_or_accepted(self):
        fetch = Mock()
        with self.assertRaisesRegex(stage.Stop, 'original sensitive value required'):
            stage.production_values([{'id':'env1','key':'KEY','target':['production'],
                                     'type':'sensitive','value':'hidden','decrypted':True}], fetch)
        fetch.assert_not_called()

    def test_original_sensitive_value_supplied_without_api_read(self):
        fetch = Mock()
        supply = Mock(return_value='original$secret ')
        self.assertEqual(stage.production_values([
            {'id':'env1','key':'KEY','target':['production'],'type':'sensitive','value':'mask'}
        ], fetch, supply), {'KEY':'original$secret '})
        supply.assert_called_once_with('KEY')
        fetch.assert_not_called()

    def test_empty_original_does_not_import_mask(self):
        with self.assertRaises(stage.Stop):
            stage.production_values([
                {'key':'KEY','target':['production'],'type':'sensitive','value':'mask'}
            ], supply_sensitive=lambda key: '')

    def test_hidden_prompt_stops_on_empty_input(self):
        with patch.object(stage.getpass, 'getpass', return_value=''), patch('builtins.print'):
            with self.assertRaisesRegex(stage.Stop, 'no services started'):
                stage.original_sensitive_value('KEY')

    def test_preview_not_fetched(self):
        fetch = Mock()
        self.assertEqual(stage.production_values([{'id':'env1','key':'KEY',
                                                  'target':['preview'],'type':'encrypted'}], fetch), {})
        fetch.assert_not_called()

    def test_mismatched_identity_rejected(self):
        fetch = Mock(return_value={'id':'env1','key':'OTHER','decrypted':True,'value':'secret'})
        with self.assertRaisesRegex(stage.Stop, 'identity mismatch'):
            stage.production_values([{'id':'env1','key':'KEY','target':['production']}], fetch)

    def test_per_variable_ciphertext_rejected(self):
        fetch = Mock(return_value={'key':'KEY','decrypted':False,'value':'ciphertext'})
        with self.assertRaisesRegex(stage.Stop, 'no verified plaintext'):
            stage.production_values([{'id':'env1','key':'KEY','target':['production']}], fetch)

    def test_permission_error_names_key_without_value(self):
        fetch = Mock(side_effect=stage.Stop('Provider request failed: HTTP 403'))
        with self.assertRaisesRegex(stage.Stop, 'KEY.*HTTP 403'):
            stage.production_values([{'id':'env1','key':'KEY','target':['production']}], fetch)

    def test_vercel_uses_dedicated_endpoint(self):
        with patch.object(stage, 'get_json', side_effect=[
            {'envs':[{'id':'env1','key':'KEY','target':['production'],'type':'encrypted'}]},
            {'id':'env1','key':'KEY','type':'encrypted','decrypted':True,'value':'original'},
        ]) as fetch:
            self.assertEqual(stage.vercel_values('token'), {'KEY':'original'})
        self.assertIn('/v1/projects/' + stage.PROJECT + '/env/env1?', fetch.call_args_list[1].args[0])
        self.assertNotIn('decrypt=', fetch.call_args_list[0].args[0])

if __name__ == '__main__':
    unittest.main()
