import pathlib
import plistlib
import tempfile
import unittest
import zipfile

from check_testflight import ipa_metadata, matching_build


class TestFlightVerificationTests(unittest.TestCase):
    def test_ipa_metadata_ignores_embedded_frameworks(self):
        metadata = {'CFBundleIdentifier': 'org.alexandria.node',
                    'CFBundleShortVersionString': '0.5.0', 'CFBundleVersion': '42'}
        with tempfile.TemporaryDirectory() as directory:
            ipa = pathlib.Path(directory) / 'app.ipa'
            with zipfile.ZipFile(ipa, 'w') as archive:
                archive.writestr('Payload/Alexandria.app/Info.plist', plistlib.dumps(metadata))
                archive.writestr('Payload/Alexandria.app/Frameworks/Other.app/Info.plist', b'ignored')
            self.assertEqual(ipa_metadata(ipa), metadata)

    def test_build_number_alone_cannot_match_another_release(self):
        response = {'data': [{'id': 'build', 'attributes': {'version': '42', 'processingState': 'VALID'},
                             'relationships': {'preReleaseVersion': {'data': {'id': 'version'}}}}],
                    'included': [{'id': 'version', 'type': 'preReleaseVersions',
                                  'attributes': {'version': '0.4.5', 'platform': 'IOS'}}]}
        self.assertIsNone(matching_build(response, '0.5.0', '42'))
        response['included'][0]['attributes']['version'] = '0.5.0'
        self.assertEqual(matching_build(response, '0.5.0', '42')['id'], 'build')
        response['included'][0]['attributes']['platform'] = 'MAC_OS'
        self.assertIsNone(matching_build(response, '0.5.0', '42'))


if __name__ == '__main__':
    unittest.main()
