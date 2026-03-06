fn main() {
    tauri_build::build();

    // Emit a custom cfg so that files shared with the CLI crate
    // (via #[path] includes) can gate test modules that depend on
    // app_lib types (Database, etc.) which don't exist in the CLI.
    println!("cargo::rustc-check-cfg=cfg(has_app_lib)");
    println!("cargo:rustc-cfg=has_app_lib");

    // On iOS, the `if-watch` crate (pulled in by libp2p's QUIC transport)
    // references macOS-only SCDynamicStore symbols from SystemConfiguration.
    // These symbols don't exist in iOS's SystemConfiguration framework.
    // We compile a small C stub that provides no-op implementations so the
    // linker succeeds. The code paths using SCDynamicStore are never reached
    // on iOS (mDNS is disabled, and if-watch falls back to polling).
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "ios" {
        let out_dir = std::env::var("OUT_DIR").unwrap();
        let stub_path = format!("{out_dir}/sc_dynamic_store_stubs.c");
        std::fs::write(
            &stub_path,
            r#"
// Stub implementations for macOS-only SCDynamicStore symbols.
// These satisfy the linker on iOS where the symbols don't exist.
// The code paths that call these are never reached on iOS.
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
"#,
        )
        .expect("failed to write SC stubs");

        cc::Build::new().file(&stub_path).compile("sc_stubs");

        // Link AudioToolbox framework for cpal/coreaudio audio I/O on iOS.
        println!("cargo:rustc-link-lib=framework=AudioToolbox");

        // Link VideoToolbox + CoreMedia + CoreVideo for H.264 encoding/decoding
        // via VTCompressionSession / VTDecompressionSession (Phase 3: mobile video).
        println!("cargo:rustc-link-lib=framework=VideoToolbox");
        println!("cargo:rustc-link-lib=framework=CoreMedia");
        println!("cargo:rustc-link-lib=framework=CoreVideo");

        // Link AVFoundation for camera capture (AVCaptureSession).
        println!("cargo:rustc-link-lib=framework=AVFoundation");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
    }
}
