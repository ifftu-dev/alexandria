import pathlib
import subprocess
import unittest


WORKFLOW = pathlib.Path(__file__).resolve().parents[1] / '.github/workflows/desktop-shared.yml'


def updater_platform(artifact):
    source = WORKFLOW.read_text()
    start = source.index('            case "$artifact" in')
    end = source.index('            esac', start) + len('            esac')
    script = 'add_platform() { printf "%s" "$1"; }\nartifact="$1"\n' + source[start:end]
    return subprocess.check_output(['bash', '-c', script, '--', artifact], text=True)


class ReleasePlatformTests(unittest.TestCase):
    def test_signed_installers_match_their_platform(self):
        artifacts = {
            'Alexandria-0.5.0-alpha-macOS-ARM64.app.tar.gz': 'darwin-aarch64',
            'Alexandria-0.5.0-alpha-Windows-x64-Setup.exe': 'windows-x86_64',
            'Alexandria_0.4.5_x64-setup.nsis.zip': 'windows-x86_64',
            'Alexandria-0.5.0-alpha-Linux-x86_64.AppImage': 'linux-x86_64',
            'Alexandria_0.5.0-alpha_amd64.AppImage': 'linux-x86_64',
            'Alexandria-0.5.0-alpha-Linux-ARM64.AppImage': 'linux-aarch64',
            'Alexandria_0.5.0-alpha_aarch64.AppImage': 'linux-aarch64',
            'Alexandria_0.4.5_aarch64.AppImage.tar.gz': 'linux-aarch64',
        }
        for artifact, platform in artifacts.items():
            with self.subTest(artifact=artifact):
                self.assertEqual(updater_platform(artifact), platform)

    def test_download_only_packages_are_not_updater_payloads(self):
        for artifact in ['Alexandria.apk', 'Alexandria.ipa', 'Alexandria.deb',
                         'Alexandria.dmg', 'Alexandria_unknown.AppImage']:
            with self.subTest(artifact=artifact):
                self.assertEqual(updater_platform(artifact), '')


if __name__ == '__main__':
    unittest.main()
