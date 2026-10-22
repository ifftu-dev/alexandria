import hashlib
import json
import pathlib
import subprocess
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]


class ResourceCheckoutTests(unittest.TestCase):
    def test_windows_checkout_preserves_embedded_resource_bytes(self):
        paths = [
            'src-tauri/resources/bootstrap_registry.json',
            'src-tauri/resources/networks/preprod.json',
            'src-tauri/resources/networks/preprod-demo-genesis.json',
            'src-tauri/resources/networks/demo-opinions-policy.json',
            'plugins/builtin/mcq/manifest.json',
            'plugins/builtin/mcq/ui/app.js',
        ]
        originals = {path: (ROOT / path).read_bytes() for path in paths}
        originals['src-tauri/resources/example.bin'] = b'\x00\x01\n\r\n\xff'
        originals['plugins/builtin/example.wasm'] = b'\x00asm\x01\x00\n\r\n'

        with tempfile.TemporaryDirectory() as directory:
            repo = pathlib.Path(directory)

            def git(*args):
                subprocess.run(['git', '-C', str(repo), *args], check=True,
                               capture_output=True)

            git('init', '--quiet')
            git('config', 'core.autocrlf', 'true')
            (repo / '.gitattributes').write_bytes((ROOT / '.gitattributes').read_bytes())
            (repo / 'control.txt').write_bytes(b'first\nsecond\n')
            for path, content in originals.items():
                target = repo / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(content)
            git('add', '.')
            for path in [*originals, 'control.txt']:
                (repo / path).unlink()
            git('checkout-index', '--all', '--force')

            self.assertEqual((repo / 'control.txt').read_bytes(), b'first\r\nsecond\r\n')
            for path, content in originals.items():
                with self.subTest(path=path):
                    self.assertEqual((repo / path).read_bytes(), content)

            profile = json.loads((repo / 'src-tauri/resources/networks/preprod.json').read_bytes())
            bootstrap = (repo / 'src-tauri/resources/bootstrap_registry.json').read_bytes()
            self.assertEqual(hashlib.sha256(bootstrap).hexdigest(),
                             profile['signed_bootstrap_registry_identity']['sha256'])


if __name__ == '__main__':
    unittest.main()
