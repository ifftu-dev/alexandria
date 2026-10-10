//! Platform capture for device attestation.
//!
//! * iOS — `DCAppAttestService` (DeviceCheck framework) through raw objc2
//!   message sends: generate a Secure Enclave key, attest it over the
//!   session nonce, and later sign the terminal commitment root with it.
//!   Completion handlers fire on a private queue; callers block on a channel
//!   from a worker thread, never from the main thread.
//! * Android — `MainActivity.requestIntegrityToken(nonce)` over JNI, which
//!   waits on the Play Integrity task and returns the encrypted token.
//! * Everything else — unsupported; the session stays `local`.
//!
//! Only the opaque attestation material comes back. No identifiers, no
//! device serials: the platform binds its statement to our app id and our
//! nonce, nothing more.

use super::{AttestationPayload, Platform};

/// What the platform handed back at session start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    pub platform: Platform,
    pub app_id: String,
    pub payload: AttestationPayload,
}

/// Whether this build can attest at all.
pub fn supported() -> bool {
    imp::supported()
}

/// Attest the session nonce. `Ok(None)` when the platform cannot attest
/// (desktop, simulator, missing Play services); `Err` for a failed attempt
/// on a platform that should have worked.
pub fn capture(nonce: &[u8; 32]) -> Result<Option<Captured>, String> {
    imp::capture(nonce)
}

/// iOS only: sign `root` with the attested key. Returns the base64
/// assertion object, or `Ok(None)` where assertions do not exist.
pub fn assert_commitment(key_id_b64: &str, root: &str) -> Result<Option<String>, String> {
    imp::assert_commitment(key_id_b64, root)
}

#[cfg(target_os = "ios")]
mod imp {
    use std::ffi::{c_char, c_void, CStr};
    use std::sync::mpsc;
    use std::time::Duration;

    use base64::Engine as _;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use sha2::{Digest, Sha256};

    use super::super::{apple_app_id, AttestationPayload, Platform};
    use super::Captured;

    #[link(name = "DeviceCheck", kind = "framework")]
    unsafe extern "C" {}

    unsafe extern "C" {
        fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
    }

    /// Nothing in the binary references a DeviceCheck symbol (every call is
    /// a message send), so a dead-stripping linker may drop the framework's
    /// load command. Load it explicitly before the first class lookup.
    fn ensure_devicecheck_loaded() -> bool {
        static LOADED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *LOADED.get_or_init(|| {
            let path = c"/System/Library/Frameworks/DeviceCheck.framework/DeviceCheck";
            // SAFETY: valid NUL-terminated path; RTLD_LAZY | RTLD_GLOBAL = 0x1 | 0x8.
            !unsafe { dlopen(path.as_ptr(), 0x1 | 0x8) }.is_null()
                && objc2::runtime::AnyClass::get(c"DCAppAttestService").is_some()
        })
    }

    const TIMEOUT: Duration = Duration::from_secs(30);

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    /// Copy an `NSString` out while it is alive (inside the block).
    unsafe fn ns_string_to_string(s: *mut AnyObject) -> Option<String> {
        if s.is_null() {
            return None;
        }
        let c: *const c_char = msg_send![&*s, UTF8String];
        if c.is_null() {
            return None;
        }
        Some(CStr::from_ptr(c).to_string_lossy().into_owned())
    }

    /// Copy an `NSData` out while it is alive (inside the block).
    unsafe fn ns_data_to_vec(d: *mut AnyObject) -> Option<Vec<u8>> {
        if d.is_null() {
            return None;
        }
        let len: usize = msg_send![&*d, length];
        let ptr: *const c_void = msg_send![&*d, bytes];
        if ptr.is_null() {
            return Some(Vec::new());
        }
        Some(std::slice::from_raw_parts(ptr as *const u8, len).to_vec())
    }

    unsafe fn ns_error_to_string(e: *mut AnyObject) -> String {
        if e.is_null() {
            return "unknown error".into();
        }
        let desc: *mut AnyObject = msg_send![&*e, localizedDescription];
        ns_string_to_string(desc).unwrap_or_else(|| "unknown error".into())
    }

    unsafe fn ns_data_from(bytes: &[u8]) -> Retained<AnyObject> {
        msg_send![class!(NSData), dataWithBytes: bytes.as_ptr() as *const c_void, length: bytes.len()]
    }

    unsafe fn ns_string_from(s: &str) -> Retained<AnyObject> {
        let bytes = s.as_bytes();
        // NSUTF8StringEncoding = 4
        let alloc: *mut AnyObject = msg_send![class!(NSString), alloc];
        let obj: *mut AnyObject = msg_send![alloc, initWithBytes: bytes.as_ptr() as *const c_void, length: bytes.len(), encoding: 4usize];
        Retained::from_raw(obj).expect("NSString init")
    }

    fn service() -> Result<Retained<AnyObject>, String> {
        if !ensure_devicecheck_loaded() {
            return Err("DeviceCheck framework unavailable".into());
        }
        // SAFETY: plain class method on DeviceCheck's singleton.
        let svc: Retained<AnyObject> =
            unsafe { msg_send![class!(DCAppAttestService), sharedService] };
        Ok(svc)
    }

    pub fn supported() -> bool {
        let Ok(svc) = service() else { return false };
        // SAFETY: BOOL getter on a live object.
        unsafe { msg_send![&*svc, isSupported] }
    }

    fn generate_key(svc: &AnyObject) -> Result<String, String> {
        let (tx, rx) = mpsc::sync_channel::<Result<String, String>>(1);
        let block = block2::RcBlock::new(move |key_id: *mut AnyObject, err: *mut AnyObject| {
            // SAFETY: both pointers are valid for the duration of this call.
            let out = unsafe {
                match ns_string_to_string(key_id) {
                    Some(k) => Ok(k),
                    None => Err(ns_error_to_string(err)),
                }
            };
            let _ = tx.send(out);
        });
        // SAFETY: `generateKeyWithCompletionHandler:` takes a
        // `void (^)(NSString *, NSError *)`; the block outlives the call
        // because the channel keeps it referenced until it fires.
        unsafe {
            let _: () = msg_send![svc, generateKeyWithCompletionHandler: &*block];
        }
        rx.recv_timeout(TIMEOUT)
            .map_err(|_| "App Attest key generation timed out".to_string())?
    }

    fn data_completion(
        svc: &AnyObject,
        selector: &str,
        key_id: &str,
        client_data_hash: &[u8],
    ) -> Result<Vec<u8>, String> {
        let (tx, rx) = mpsc::sync_channel::<Result<Vec<u8>, String>>(1);
        let block = block2::RcBlock::new(move |data: *mut AnyObject, err: *mut AnyObject| {
            // SAFETY: both pointers are valid for the duration of this call.
            let out = unsafe {
                match ns_data_to_vec(data) {
                    Some(d) if !d.is_empty() => Ok(d),
                    _ => Err(ns_error_to_string(err)),
                }
            };
            let _ = tx.send(out);
        });
        // SAFETY: both selectors share the shape
        // `-(void)xxx:(NSString *)keyId clientDataHash:(NSData *)hash completionHandler:(void (^)(NSData *, NSError *))h`.
        unsafe {
            let key = ns_string_from(key_id);
            let hash = ns_data_from(client_data_hash);
            match selector {
                "attest" => {
                    let _: () = msg_send![svc, attestKey: &*key, clientDataHash: &*hash, completionHandler: &*block];
                }
                _ => {
                    let _: () = msg_send![svc, generateAssertion: &*key, clientDataHash: &*hash, completionHandler: &*block];
                }
            }
        }
        rx.recv_timeout(TIMEOUT)
            .map_err(|_| format!("App Attest {selector} timed out"))?
    }

    pub fn capture(nonce: &[u8; 32]) -> Result<Option<Captured>, String> {
        let svc = service()?;
        if !supported() {
            return Ok(None);
        }
        let key_id = generate_key(&svc)?;
        let attestation = data_completion(&svc, "attest", &key_id, nonce)?;
        Ok(Some(Captured {
            platform: Platform::Ios,
            app_id: apple_app_id(),
            payload: AttestationPayload::AppAttest {
                key_id_b64: key_id,
                attestation_b64: b64(&attestation),
                assertion_b64: None,
                asserted_root: None,
            },
        }))
    }

    pub fn assert_commitment(key_id_b64: &str, root: &str) -> Result<Option<String>, String> {
        let svc = service()?;
        if !supported() {
            return Ok(None);
        }
        let client_data_hash: [u8; 32] = Sha256::digest(root.as_bytes()).into();
        let assertion = data_completion(&svc, "assert", key_id_b64, &client_data_hash)?;
        Ok(Some(b64(&assertion)))
    }
}

#[cfg(target_os = "android")]
mod imp {
    use base64::Engine as _;

    use super::super::{AttestationPayload, Platform, APP_BUNDLE_ID};
    use super::Captured;

    pub fn supported() -> bool {
        true
    }

    pub fn capture(nonce: &[u8; 32]) -> Result<Option<Captured>, String> {
        let nonce_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(nonce);
        let token = crate::av_permissions::jni::with_activity_class(|env, class| {
            let arg = env.new_string(&nonce_b64)?;
            let out = env
                .call_static_method(
                    class,
                    "requestIntegrityToken",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[jni::objects::JValue::Object(&arg)],
                )
                .and_then(|v| v.l())?;
            let s: String = env.get_string(&jni::objects::JString::from(out))?.into();
            Ok(s)
        })?;
        if token.is_empty() {
            // Play services absent or the request failed; the Kotlin side
            // already logged the cause. Not an error: the session stays local.
            return Ok(None);
        }
        Ok(Some(Captured {
            platform: Platform::Android,
            app_id: APP_BUNDLE_ID.to_string(),
            payload: AttestationPayload::PlayIntegrity { token },
        }))
    }

    pub fn assert_commitment(_key_id_b64: &str, _root: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
mod imp {
    use super::Captured;

    pub fn supported() -> bool {
        false
    }

    pub fn capture(_nonce: &[u8; 32]) -> Result<Option<Captured>, String> {
        Ok(None)
    }

    pub fn assert_commitment(_key_id_b64: &str, _root: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
}
