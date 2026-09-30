use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PersonhoodLabAction {
    Download,
    Prove,
    Cancel,
    RemoveKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonhoodLabResult {
    pub elapsed_ms: f64,
    pub peak_rss_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonhoodLabStatus {
    pub enabled: bool,
    pub phase: String,
    pub key_status: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub elapsed_ms: u64,
    pub error: Option<String>,
    pub result: Option<PersonhoodLabResult>,
}

fn enabled() -> bool {
    cfg!(all(
        feature = "personhood-lab",
        debug_assertions,
        target_os = "android"
    ))
}

#[tauri::command]
pub async fn personhood_lab_status() -> Result<PersonhoodLabStatus, String> {
    if !enabled() {
        return Ok(PersonhoodLabStatus {
            enabled: false,
            phase: "idle".into(),
            key_status: "missing".into(),
            downloaded_bytes: 0,
            total_bytes: 612_082_146,
            elapsed_ms: 0,
            error: None,
            result: None,
        });
    }
    invoke_native("status")
}

#[tauri::command]
pub async fn personhood_lab_action(
    action: PersonhoodLabAction,
) -> Result<PersonhoodLabStatus, String> {
    if !enabled() {
        return Err("Personhood Lab requires an explicitly enabled Android debug build".into());
    }
    let action = match action {
        PersonhoodLabAction::Download => "download",
        PersonhoodLabAction::Prove => "prove",
        PersonhoodLabAction::Cancel => "cancel",
        PersonhoodLabAction::RemoveKey => "remove_key",
    };
    invoke_native(action)
}

#[cfg(not(target_os = "android"))]
fn invoke_native(_action: &str) -> Result<PersonhoodLabStatus, String> {
    Err("Personhood Lab is only available on Android".into())
}

#[cfg(target_os = "android")]
fn invoke_native(action: &str) -> Result<PersonhoodLabStatus, String> {
    use jni::objects::{JClass, JObject, JString, JValue};

    let ctx = ndk_context::android_context();
    // ndk_context retains the VM and application global reference for process lifetime.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.map_err(|e| e.to_string())?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let json = env
        .with_local_frame(16, |env| -> jni::errors::Result<String> {
            let raw = env.get_native_interface();
            let local = unsafe {
                let table = (*raw)
                    .as_ref()
                    .ok_or(jni::errors::Error::NullPtr("JNIEnv"))?;
                let new_ref = table
                    .NewLocalRef
                    .ok_or(jni::errors::Error::JNIEnvMethodNotFound("NewLocalRef"))?;
                new_ref(raw, ctx.context().cast())
            };
            if local.is_null() {
                return Err(jni::errors::Error::NullPtr("Application"));
            }
            let app = unsafe { JObject::from_raw(local) };
            let loader = env
                .call_method(&app, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
                .l()?;
            let name = env.new_string("org.alexandria.node.MainActivity")?;
            let class = JClass::from(
                env.call_method(
                    &loader,
                    "loadClass",
                    "(Ljava/lang/String;)Ljava/lang/Class;",
                    &[JValue::Object(&name)],
                )?
                .l()?,
            );
            let argument = env.new_string(action)?;
            let response = JString::from(
                env.call_static_method(
                    &class,
                    "personhoodLab",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[JValue::Object(&argument)],
                )?
                .l()?,
            );
            let result: String = env.get_string(&response)?.into();
            Ok(result)
        })
        .map_err(|e| format!("Personhood Lab bridge: {e}"))?;
    serde_json::from_str(&json).map_err(|e| format!("Invalid Personhood Lab status: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unavailable_build_cannot_start_mutations() {
        if !enabled() {
            assert!(!personhood_lab_status().await.unwrap().enabled);
            for action in [
                PersonhoodLabAction::Download,
                PersonhoodLabAction::Prove,
                PersonhoodLabAction::Cancel,
                PersonhoodLabAction::RemoveKey,
            ] {
                assert!(personhood_lab_action(action).await.is_err());
            }
        }
    }

    #[test]
    fn arbitrary_paths_urls_and_identity_inputs_are_not_actions() {
        for value in ["https://example.com/key", "../../worker", "verify_identity"] {
            assert!(
                serde_json::from_value::<PersonhoodLabAction>(serde_json::json!(value)).is_err()
            );
        }
    }
}
