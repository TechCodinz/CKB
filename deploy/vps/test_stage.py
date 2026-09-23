import unittest
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

if __name__ == '__main__':
    unittest.main()
