//! Camera and microphone permission, asked for at the moment they are used.
//!
//! Only Android needs anything here. iOS prompts on its own the first time
//! AVFoundation opens a device, and desktop platforms either do the same or
//! have no runtime permission model. On Android the native A/V pipeline —
//! cpal for the mic, the NDK Camera2 API for video — bypasses the WebView's
//! `getUserMedia` permission handler, so nothing would ever ask, and a denied
//! microphone used to abort the process from inside cpal. The app used to
//! request both at launch to avoid that; this asks at first use instead and
//! refuses cleanly on no.
//!
//! The Kotlin half lives in `gen/android/.../MainActivity.kt`: two static
//! methods, reached through the app's class loader because `ndk_context`
//! holds the Application and `FindClass` from a Rust thread cannot see app
//! classes.

/// Make sure camera and microphone may be opened, prompting if needed.
///
/// Resolves once the user has answered. `Err` carries a message fit for the
/// session-start error path.
#[cfg(not(target_os = "android"))]
pub async fn ensure_camera_and_microphone() -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "android")]
pub async fn ensure_camera_and_microphone() -> Result<(), String> {
    use std::time::Duration;

    const GRANTED: i32 = 1;
    const PENDING: i32 = 0;
    /// Long enough to read the dialog; a user who walks away gets a clean
    /// error rather than a session that opens minutes later.
    const WAIT: Duration = Duration::from_secs(120);
    const POLL: Duration = Duration::from_millis(200);

    if jni::state()? == GRANTED {
        return Ok(());
    }
    jni::request()?;
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        tokio::time::sleep(POLL).await;
        match jni::state()? {
            GRANTED => return Ok(()),
            PENDING if tokio::time::Instant::now() < deadline => continue,
            PENDING => {
                return Err("timed out waiting for the camera and microphone permission".into())
            }
            _ => {
                return Err(
                    "Camera and microphone access is needed for a live session. \
                            Allow both for Alexandria in Android's app settings and try again."
                        .into(),
                )
            }
        }
    }
}

#[cfg(target_os = "android")]
mod jni {
    use jni::objects::{JClass, JObject, JValue};

    const ACTIVITY: &str = "org.alexandria.node.MainActivity";

    fn with_activity_class<T>(
        f: impl FnOnce(&mut ::jni::JNIEnv, &JClass) -> ::jni::errors::Result<T>,
    ) -> Result<T, String> {
        let ctx = ndk_context::android_context();
        // SAFETY: `ndk_context` was initialised in JNI_OnLoad with the
        // process JavaVM and a leaked global ref to the Application; both
        // stay valid for the life of the process.
        let vm = unsafe { ::jni::JavaVM::from_raw(ctx.vm().cast()) }.map_err(|e| e.to_string())?;
        let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
        let app = unsafe { JObject::from_raw(ctx.context().cast()) };
        let loader = env
            .call_method(&app, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
            .and_then(|v| v.l())
            .map_err(|e| e.to_string())?;
        let name = env.new_string(ACTIVITY).map_err(|e| e.to_string())?;
        let class = env
            .call_method(
                &loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&name)],
            )
            .and_then(|v| v.l())
            .map_err(|e| e.to_string())?;
        let class = JClass::from(class);
        f(&mut env, &class).map_err(|e| format!("{ACTIVITY}: {e}"))
    }

    pub fn state() -> Result<i32, String> {
        with_activity_class(|env, class| {
            env.call_static_method(class, "avPermissionState", "()I", &[])
                .and_then(|v| v.i())
        })
    }

    pub fn request() -> Result<(), String> {
        with_activity_class(|env, class| {
            env.call_static_method(class, "requestAvPermissions", "()V", &[])
                .map(|_| ())
        })
    }
}
