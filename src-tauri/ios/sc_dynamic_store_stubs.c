// Stub implementations for macOS-only SCDynamicStore symbols.
// These satisfy the linker on iOS where the symbols don't exist.
// They return NULL, so any caller that wraps the result without checking
// panics: hickory's system DNS reader did (org_trust.rs avoids it on iOS).
#include <stddef.h>
#include <stdint.h>

typedef const void *CFTypeRef;
typedef CFTypeRef CFStringRef;
typedef CFTypeRef CFArrayRef;
typedef CFTypeRef CFAllocatorRef;
typedef CFTypeRef CFDictionaryRef;
typedef CFTypeRef CFRunLoopSourceRef;

void *SCDynamicStoreCreateWithOptions(CFAllocatorRef allocator, CFStringRef name, uint32_t options, void *callout, void *context) {
    return NULL;
}

void *SCDynamicStoreCreateRunLoopSource(CFAllocatorRef allocator, void *store, int32_t order) {
    return NULL;
}

int SCDynamicStoreSetNotificationKeys(void *store, CFArrayRef keys, CFArrayRef patterns) {
    return 0;
}

uint32_t kSCDynamicStoreUseSessionKeys = 0;

// --- Additional macOS-only SystemConfiguration symbols referenced by the
// `netdev` crate (pulled in by iroh 1.0 for interface discovery). The
// SCNetworkService / SCPreferences / SCNetworkInterface APIs don't exist on
// iOS; stub them so linking succeeds. iroh falls back to other discovery.
void *SCDynamicStoreCreate(CFAllocatorRef a, CFStringRef n, void *c, void *ctx) { return NULL; }
void *SCDynamicStoreCopyValue(void *store, CFStringRef key) { return NULL; }
void *SCDynamicStoreKeyCreateNetworkGlobalEntity(CFAllocatorRef a, CFStringRef domain, CFStringRef entity) { return NULL; }
void *SCDynamicStoreKeyCreateNetworkServiceEntity(CFAllocatorRef a, CFStringRef domain, CFStringRef svc, CFStringRef entity) { return NULL; }
void *SCPreferencesCreate(CFAllocatorRef a, CFStringRef name, CFStringRef prefsID) { return NULL; }
void *SCNetworkServiceCopyAll(void *prefs) { return NULL; }
void *SCNetworkServiceGetInterface(void *service) { return NULL; }
CFStringRef SCNetworkServiceGetName(void *service) { return NULL; }
void *SCNetworkServiceCopyProtocol(void *service, CFStringRef protocolType) { return NULL; }
void *SCNetworkInterfaceCopyAll(void) { return NULL; }
CFStringRef SCNetworkInterfaceGetBSDName(void *iface) { return NULL; }
CFStringRef SCNetworkInterfaceGetInterfaceType(void *iface) { return NULL; }
CFStringRef SCNetworkInterfaceGetLocalizedDisplayName(void *iface) { return NULL; }
CFDictionaryRef SCNetworkProtocolGetConfiguration(void *protocol) { return NULL; }

// `system-configuration` 0.6 (netwatch / iroh dependency refresh) also calls
// the macOS-only proxy and network-set APIs. Keep these fallbacks separate
// from SCNetworkReachability: those APIs exist on iOS and Firebase consumes
// their real objects. Overriding their constructors with NULL-returning
// stubs makes Firebase crash when it registers a native callback.
CFDictionaryRef SCDynamicStoreCopyProxies(void *store) { return NULL; }
unsigned char SCNetworkServiceGetEnabled(void *service) { return 0; }
CFStringRef SCNetworkServiceGetServiceID(void *service) { return NULL; }
void *SCNetworkSetCopyCurrent(void *prefs) { return NULL; }
void *SCNetworkSetGetServiceOrder(void *set) { return NULL; }

const CFStringRef kSCDynamicStoreDomainState = NULL;
const CFStringRef kSCDynamicStorePropNetPrimaryService = NULL;
const CFStringRef kSCEntNetDNS = NULL;
const CFStringRef kSCEntNetIPv4 = NULL;
const CFStringRef kSCNetworkProtocolTypeIPv4 = NULL;
const CFStringRef kSCNetworkProtocolTypeIPv6 = NULL;
const CFStringRef kSCPropNetDNSServerAddresses = NULL;
// `system-configuration` 0.6 matches every kSCNetworkInterfaceType* constant in
// `from_cfstring`. The iOS 27 SDK's SystemConfiguration.tbd exports them for
// arm64e only, so an arm64 link cannot resolve them from the framework; NULL
// stubs turn the match into a no-match (iroh never needs interface types).
const CFStringRef kSCNetworkInterfaceType6to4 = NULL;
const CFStringRef kSCNetworkInterfaceTypeBluetooth = NULL;
const CFStringRef kSCNetworkInterfaceTypeBond = NULL;
const CFStringRef kSCNetworkInterfaceTypeBridge = NULL;
const CFStringRef kSCNetworkInterfaceTypeEthernet = NULL;
const CFStringRef kSCNetworkInterfaceTypeFireWire = NULL;
const CFStringRef kSCNetworkInterfaceTypeIEEE80211 = NULL;
const CFStringRef kSCNetworkInterfaceTypeIPSec = NULL;
const CFStringRef kSCNetworkInterfaceTypeIPv4 = NULL;
const CFStringRef kSCNetworkInterfaceTypeIrDA = NULL;
const CFStringRef kSCNetworkInterfaceTypeL2TP = NULL;
const CFStringRef kSCNetworkInterfaceTypeModem = NULL;
const CFStringRef kSCNetworkInterfaceTypePPP = NULL;
const CFStringRef kSCNetworkInterfaceTypePPTP = NULL;
const CFStringRef kSCNetworkInterfaceTypeSerial = NULL;
const CFStringRef kSCNetworkInterfaceTypeVLAN = NULL;
const CFStringRef kSCNetworkInterfaceTypeWWAN = NULL;
