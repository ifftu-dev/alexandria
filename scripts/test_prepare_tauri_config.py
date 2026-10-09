import json
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


class PrepareTauriConfigTests(unittest.TestCase):
    def test_version_sync_keeps_locked_sidecar_build_consistent(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            files = ['package.json', 'package-lock.json', 'Cargo.lock',
                     'src-tauri/Cargo.toml', 'src-tauri/tauri.conf.json']
            for name in files:
                dest = root / name
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / name, dest)
            before = (root / 'Cargo.lock').read_text()
            version = '0.6.1-dev-seeds.20261003.2'
            subprocess.run(['node', str(ROOT / 'scripts/prepare-tauri-config.mjs'),
                            'sync-version', version], cwd=root, check=True, capture_output=True)
            after = (root / 'Cargo.lock').read_text()
            expected = re.sub(r'(\[\[package\]\]\nname = "alexandria-node"\nversion = ")[^"]+("[^\n]*)',
                              lambda m: m[1] + version + m[2], before)
            self.assertEqual(after, expected)
            self.assertNotEqual(after, before)
            self.assertIn(f'version = "{version}"', (root / 'src-tauri/Cargo.toml').read_text())
            self.assertEqual(json.loads((root / 'src-tauri/tauri.conf.json').read_text())['version'], version)

    def test_validation_uses_ad_hoc_signing_without_public_updater(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config_path = root / 'src-tauri/tauri.conf.json'
            config_path.parent.mkdir(parents=True)
            shutil.copyfile(ROOT / 'src-tauri/tauri.conf.json', config_path)
            original = json.loads(config_path.read_text())
            subprocess.run(['node', str(ROOT / 'scripts/prepare-tauri-config.mjs'),
                            'desktop-validation'], cwd=root, check=True, capture_output=True)
            config = json.loads(config_path.read_text())
            self.assertEqual(config['bundle']['macOS']['signingIdentity'], '-')
            self.assertFalse(config['bundle']['createUpdaterArtifacts'])
            self.assertEqual(config['plugins']['updater']['endpoints'], [])
            self.assertEqual(config['plugins']['updater']['pubkey'], original['plugins']['updater']['pubkey'])
            self.assertEqual(config['bundle']['macOS']['entitlements'], original['bundle']['macOS']['entitlements'])

    def test_bad_version_leaves_files_untouched(self):
        with tempfile.TemporaryDirectory() as tmp:
            result = subprocess.run(['node', str(ROOT / 'scripts/prepare-tauri-config.mjs'),
                                     'sync-version', 'not-a-version'], cwd=tmp, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(list(Path(tmp).iterdir()), [])


if __name__ == '__main__':
    unittest.main()
