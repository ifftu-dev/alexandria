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

    fn local_ref_from_global<'local>(
        env: &::jni::JNIEnv<'local>,
        global: ::jni::sys::jobject,
    ) -> ::jni::errors::Result<JObject<'local>> {
        if global.is_null() {
            return Err(::jni::errors::Error::NullPtr("Android application context"));
        }
        let interface = env.get_native_interface();
        if interface.is_null() {
            return Err(::jni::errors::Error::NullPtr("JNIEnv"));
        }
        // SAFETY: `interface` belongs to the attached current thread and
        // `global` is the process-lifetime NewGlobalRef retained by JNI_OnLoad.
        // NewLocalRef returns a reference owned by the local frame below, which
        // is exactly the reference kind JObject::from_raw requires.
        let local = unsafe {
            let table = (*interface)
                .as_ref()
                .ok_or(::jni::errors::Error::NullDeref("JNIEnv function table"))?;
            let new_local_ref = table
                .NewLocalRef
                .ok_or(::jni::errors::Error::JNIEnvMethodNotFound("NewLocalRef"))?;
            new_local_ref(interface, global)
        };
        if local.is_null() {
            return Err(::jni::errors::Error::NullPtr("NewLocalRef"));
        }
        // SAFETY: NewLocalRef returned a unique live local reference in the
        // current thread's active local frame.
        Ok(unsafe { JObject::from_raw(local) })
    }

    fn with_activity_class<T>(
        f: impl FnOnce(&mut ::jni::JNIEnv, &JClass) -> ::jni::errors::Result<T>,
    ) -> Result<T, String> {
        let ctx = ndk_context::android_context();
        // SAFETY: `ndk_context` was initialised in JNI_OnLoad with the
        // process JavaVM and a leaked global ref to the Application; both
        // stay valid for the life of the process.
        let vm = unsafe { ::jni::JavaVM::from_raw(ctx.vm().cast()) }.map_err(|e| e.to_string())?;
        let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
        env.with_local_frame(8, |env| {
            let app = local_ref_from_global(env, ctx.context().cast())?;
            let loader = env
                .call_method(&app, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
                .and_then(|v| v.l())?;
            let name = env.new_string(ACTIVITY)?;
            let class = env
                .call_method(
                    &loader,
                    "loadClass",
                    "(Ljava/lang/String;)Ljava/lang/Class;",
                    &[JValue::Object(&name)],
                )
                .and_then(|v| v.l())?;
            let class = JClass::from(class);
            f(env, &class)
        })
        .map_err(|e| format!("{ACTIVITY}: {e}"))
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
