import json
import os
import pathlib
import plistlib
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile


def ipa_metadata(path):
    with zipfile.ZipFile(path) as archive:
        names = [name for name in archive.namelist()
                 if name.startswith('Payload/') and name.endswith('.app/Info.plist')
                 and name.count('/') == 2]
        if len(names) != 1:
            raise ValueError('Expected exactly one application Info.plist in the IPA')
        info = plistlib.loads(archive.read(names[0]))
    return {key: str(info[key]) for key in
            ('CFBundleIdentifier', 'CFBundleShortVersionString', 'CFBundleVersion')}


def matching_build(response, version, build_number):
    included = {item['id']: item for item in response.get('included', [])
                if item['type'] == 'preReleaseVersions'}
    for build in response.get('data', []):
        related = build.get('relationships', {}).get('preReleaseVersion', {}).get('data')
        prerelease = included.get(related['id'], {}) if related else {}
        attributes = prerelease.get('attributes', {})
        if (build['attributes']['version'] == build_number
                and attributes.get('version') == version
                and attributes.get('platform') == 'IOS'):
            return build
    return None


def main():
    import jwt

    metadata = ipa_metadata(sys.argv[1])
    key = pathlib.Path(os.environ['APPLE_API_KEY_PATH']).read_text()

    def get(resource, params=None):
        token = jwt.encode(
            {'iss': os.environ['APPLE_API_ISSUER'], 'aud': 'appstoreconnect-v1',
             'exp': int(time.time()) + 300},
            key, algorithm='ES256', headers={'kid': os.environ['APPLE_API_KEY']},
        )
        url = 'https://api.appstoreconnect.apple.com/v1/' + resource
        if params:
            url += '?' + urllib.parse.urlencode(params)
        request = urllib.request.Request(url, headers={'Authorization': 'Bearer ' + token})
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)

    apps = get('apps', {'filter[bundleId]': metadata['CFBundleIdentifier']})['data']
    if len(apps) != 1:
        raise RuntimeError('Could not uniquely resolve the app in App Store Connect')
    version = metadata['CFBundleShortVersionString']
    number = metadata['CFBundleVersion']
    print(f'Checking Apple processing for {version} ({number})', flush=True)
    deadline = time.monotonic() + 900
    while time.monotonic() < deadline:
        try:
            response = get('builds', {
                'filter[app]': apps[0]['id'], 'filter[version]': number,
                'include': 'preReleaseVersion,buildBetaDetail', 'limit': '200',
            })
        except urllib.error.HTTPError as error:
            if error.code != 429 and error.code < 500:
                raise
            print(f'Apple API temporarily unavailable (HTTP {error.code})', flush=True)
            time.sleep(30)
            continue
        build = matching_build(response, version, number)
        state = build['attributes']['processingState'] if build else 'AWAITING_BUILD'
        print(f'Apple processing: {state}', flush=True)
        if state in ('FAILED', 'INVALID'):
            raise RuntimeError(f'Apple rejected the uploaded build: {state}')
        if state == 'VALID':
            details = [item['attributes'] for item in response.get('included', [])
                       if item['type'] == 'buildBetaDetails'
                       and item['id'] == (build.get('relationships', {}).get('buildBetaDetail', {}).get('data') or {}).get('id')]
            print(json.dumps({'app_store_connect_build_id': build['id'],
                              'version': version, 'build_number': number,
                              'processing_state': state, 'beta_details': details}), flush=True)
            return
        time.sleep(30)
    raise TimeoutError('Upload succeeded, but Apple processing is still pending after 15 minutes')


if __name__ == '__main__':
    main()
