fn main() {
    tauri_build::build();

    // Re-run when any embedded plugin asset or bootstrap script changes
    // — these are pulled in via include_bytes!/include_str! and Cargo
    // doesn't track them by default.
    println!("cargo:rerun-if-changed=src/plugins/bootstrap.js");
    println!("cargo:rerun-if-changed=../plugins/builtin");

    // On iOS, the `if-watch` crate (pulled in by libp2p's QUIC transport)
    // references macOS-only SCDynamicStore symbols from SystemConfiguration.
    // These symbols don't exist in iOS's SystemConfiguration framework.
    // We compile a small C stub that provides no-op implementations so the
    // linker succeeds. if-watch never reaches them on iOS (mDNS is disabled,
    // and it falls back to polling); anything else that does gets NULL back.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // The Wasmtime grader sandbox now runs on every platform. Desktop and
    // Android execute Cranelift's native machine code directly (JIT). iOS
    // forbids JIT — Cranelift output can never be mapped executable — so iOS
    // targets Wasmtime's Pulley bytecode interpreter instead (see
    // `grader_config` in plugins/wasm_runtime.rs, and the iOS-only `pulley`
    // feature on the wasmtime dependency in Cargo.toml). Pulley needs no
    // executable pages, so the sandbox is available there too.
    //
    // We keep the single named `grader` cfg — rather than a parallel
    // `grader_pulley` — because the interpreter/JIT choice is entirely
    // internal to `grader_config`: every call site (AppState field, IPC
    // command, precompile step) is identical on all platforms. One cfg keeps
    // that machinery in one shape everywhere; the only per-target branch is
    // the Engine's target triple. `grader` is now set unconditionally and is
    // retained as the compile-in switch for the whole grader subsystem.
    println!("cargo::rustc-check-cfg=cfg(grader)");
    println!("cargo:rustc-cfg=grader");

    // `tutoring_video`: this target's video feature is on, so the media
    // crate's video types exist and the platform adapter publishes and
    // watches video. iOS has its own (VideoToolbox); desktop and Android
    // share the ffmpeg ones, exactly as the two former managers gated it.
    // Without it every platform still makes real audio-only calls. Which
    // adapter fills the seam is decided per target in
    // `tutoring/media/mod.rs`.
    println!("cargo::rustc-check-cfg=cfg(tutoring_video)");
    let has_feature = |f: &str| std::env::var(format!("CARGO_FEATURE_{f}")).is_ok();
    let video = if target_os == "ios" {
        has_feature("TUTORING_VIDEO_IOS")
    } else {
        has_feature("TUTORING_VIDEO") || has_feature("TUTORING_VIDEO_STATIC")
    };
    if video {
        println!("cargo:rustc-cfg=tutoring_video");
    }

    if target_os == "macos" {
        // Carbon provides IsSecureEventInputEnabled / DisableSecureEventInput,
        // used to clear a WKWebView Secure Event Input leak that otherwise
        // blocks global hotkey tools while the app is foreground.
        println!("cargo:rustc-link-lib=framework=Carbon");
    }
    if target_os == "ios" {
        println!("cargo:rerun-if-changed=ios/sc_dynamic_store_stubs.c");
        cc::Build::new()
            .file("ios/sc_dynamic_store_stubs.c")
            .compile("sc_stubs");

        // Link AudioToolbox framework for cpal/coreaudio audio I/O on iOS.
        // SystemConfiguration for the SCNetworkReachability symbols the
        // `system-configuration` sys crate uses; its own link attribute is
        // macOS-only.
        println!("cargo:rustc-link-lib=framework=SystemConfiguration");
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
