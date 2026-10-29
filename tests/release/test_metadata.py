import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location('prepare_release', Path(__file__).parents[2] / 'scripts/prepare-release.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
VERSION = json.loads((module.ROOT / 'package.json').read_text())['version']

class MetadataTests(unittest.TestCase):
    def test_release_requires_matching_version_and_complete_signed_artifacts(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            with self.assertRaises(ValueError):
                module.prepare(root, root / 'out', 'v999.0.0', 'Notes')
            with self.assertRaises(ValueError):
                module.prepare(root, root / 'out', f'v{VERSION}', 'Notes')

    def test_both_architectures_reference_the_same_signed_universal_build(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'macos').mkdir()
            (root / 'dmg').mkdir()
            (root / 'macos/Tasks.app.tar.gz').write_bytes(b'fixture archive')
            (root / 'macos/Tasks.app.tar.gz.sig').write_text('fixture signature')
            (root / f'dmg/Tasks_{VERSION}_universal.dmg').write_bytes(b'fixture dmg')
            out = root / 'out'
            module.prepare(root, out, f'v{VERSION}', 'Actual release notes')
            metadata = json.loads((out / 'latest.json').read_text())
            self.assertEqual(set(metadata['platforms']), {'darwin-aarch64', 'darwin-x86_64'})
            arm = metadata['platforms']['darwin-aarch64']
            self.assertEqual(arm, metadata['platforms']['darwin-x86_64'])
            self.assertEqual(arm['signature'], 'fixture signature')
            self.assertEqual(metadata['notes'], 'Actual release notes')
            cask = (out / 'tasks.rb').read_text()
            self.assertIn(module.hashlib.sha256(b'fixture dmg').hexdigest(), cask)
            self.assertNotIn('zap', cask)
            self.assertNotIn('xattr', cask)
            self.assertIn('latest.json', (out / 'SHA256SUMS').read_text())

if __name__ == '__main__': unittest.main()
