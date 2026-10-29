import io
import json
import pathlib
import plistlib
import tempfile
import unittest
from unittest.mock import Mock, patch
import urllib.error
import zipfile

from check_testflight import ipa_metadata, matching_build, main


class TestFlightVerificationTests(unittest.TestCase):
    def http_error(self, code):
        error = urllib.error.HTTPError('test', code, 'API error', {}, io.BytesIO())
        self.addCleanup(error.close)
        return error

    def run_checker(self, responses):
        clock = [0]

        def sleep(seconds):
            clock[0] += seconds

        def open_response(*args, **kwargs):
            response = next(responses)
            if isinstance(response, Exception):
                raise response
            return io.BytesIO(json.dumps(response).encode())

        with patch.dict('sys.modules', {'jwt': Mock(encode=Mock(return_value='test-token'))}), \
                patch.dict('os.environ', {'APPLE_API_KEY_PATH': 'test-key',
                                         'APPLE_API_ISSUER': 'issuer', 'APPLE_API_KEY': 'key'}), \
                patch('sys.argv', ['check_testflight.py', 'test.ipa']), \
                patch('check_testflight.ipa_metadata', return_value={
                    'CFBundleIdentifier': 'org.alexandria.node',
                    'CFBundleShortVersionString': '0.9.2', 'CFBundleVersion': '115.1'}), \
                patch('pathlib.Path.read_text', return_value='test-key'), \
                patch('check_testflight.urllib.request.urlopen', side_effect=open_response), \
                patch('check_testflight.time.monotonic', side_effect=lambda: clock[0]), \
                patch('check_testflight.time.sleep', side_effect=sleep), \
                patch('builtins.print'):
            main()
        return clock[0]

    def build_response(self, state='VALID'):
        return {'data': [{'id': 'build', 'attributes': {'version': '115.1', 'processingState': state},
                          'relationships': {'preReleaseVersion': {'data': {'id': 'version'}}}}],
                'included': [{'id': 'version', 'type': 'preReleaseVersions',
                              'attributes': {'version': '0.9.2', 'platform': 'IOS'}}]}

    def test_temporary_api_failures_retry_until_matching_build_is_valid(self):
        for error in (TimeoutError('The read operation timed out'),
                      urllib.error.URLError('connection reset'),
                      self.http_error(429),
                      self.http_error(503)):
            with self.subTest(error=type(error).__name__):
                elapsed = self.run_checker(iter([
                    {'data': [{'id': 'app'}]}, error, self.build_response()]))
                self.assertEqual(elapsed, 30)

    def test_transport_failure_cannot_extend_processing_deadline(self):
        responses = iter([{'data': [{'id': 'app'}]}] + [TimeoutError('timeout')] * 31)
        with self.assertRaisesRegex(TimeoutError, 'still pending after 15 minutes'):
            self.run_checker(responses)

    def test_authentication_and_build_rejection_still_fail(self):
        with self.assertRaises(urllib.error.HTTPError):
            self.run_checker(iter([{'data': [{'id': 'app'}]},
                                  self.http_error(401)]))
        for state in ('FAILED', 'INVALID'):
            with self.subTest(state=state), self.assertRaisesRegex(RuntimeError, 'Apple rejected'):
                self.run_checker(iter([{'data': [{'id': 'app'}]}, self.build_response(state)]))

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
