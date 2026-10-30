import os
import pathlib
import subprocess
import sys
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]


@unittest.skipUnless(sys.platform == 'darwin', 'requires Apple frameworks')
class IOSSystemConfigurationTests(unittest.TestCase):
    def test_compatibility_shim_preserves_native_reachability(self):
        # Link the same compatibility shim as the iOS app. Firebase calls
        # CreateWithAddress followed by SetCallback during startup; an app
        # definition that shadows Apple's constructor returns NULL and crashes.
        with tempfile.TemporaryDirectory() as directory:
            directory = pathlib.Path(directory)
            probe = directory / 'probe.c'
            probe.write_text(r'''
#include <SystemConfiguration/SystemConfiguration.h>
#include <netinet/in.h>
#include <stdio.h>

static void callback(SCNetworkReachabilityRef target,
                     SCNetworkReachabilityFlags flags, void *info) {}

int main(void) {
    struct sockaddr_in address = {0};
    address.sin_len = sizeof(address);
    address.sin_family = AF_INET;
    SCNetworkReachabilityRef target = SCNetworkReachabilityCreateWithAddress(
        kCFAllocatorDefault, (const struct sockaddr *)&address);
    if (!target) {
        fputs("Reachability constructor returned NULL before Firebase callback setup\n", stderr);
        return 1;
    }
    if (!SCNetworkReachabilitySetCallback(target, callback, NULL)) return 2;
    SCNetworkReachabilitySetCallback(target, NULL, NULL);
    CFRelease(target);
    return 0;
}
''')
            env = os.environ.copy()
            env.pop('IPHONEOS_DEPLOYMENT_TARGET', None)
            binary = directory / 'probe'
            subprocess.run([
                'xcrun', '--sdk', 'macosx', 'clang',
                str(probe), str(ROOT / 'src-tauri/ios/sc_dynamic_store_stubs.c'),
                '-framework', 'SystemConfiguration', '-framework', 'CoreFoundation',
                '-Wno-deprecated-declarations', '-o', str(binary),
            ], check=True, capture_output=True, text=True, env=env)
            result = subprocess.run([str(binary)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == '__main__':
    unittest.main()
