//! Cross-platform microphone permission helper for Dioxus mobile apps.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicPermission {
    Granted,
    NotDetermined,
    /// May be permanent — send the user to system Settings.
    Denied,
}

pub fn status() -> MicPermission { imp::status() }

/// Ask for the mic if needed. Resolves to the final granted state.
pub async fn ensure() -> bool { imp::ensure().await }

// ---------------------------------------------------------------- Android --
#[cfg(target_os = "android")]
mod imp {
    use super::MicPermission;
    use futures_timer::Delay;
    use jni::objects::{JObject, JValue};
    use jni::JavaVM;
    use std::time::Duration;

    const RECORD_AUDIO: &str = "android.permission.RECORD_AUDIO";
    const PERMISSION_GRANTED: i32 = 0; // PackageManager.PERMISSION_GRANTED

    /// Run `f` with a JNIEnv for this thread and the app's Activity.
    fn with_activity<R>(f: impl FnOnce(&mut jni::JNIEnv, &JObject) -> R) -> R {
        // tao/winit (via android-activity) initializes ndk-context before main().
        let ctx = ndk_context::android_context();
        let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }.expect("JavaVM");
        let mut env = vm.attach_current_thread().expect("attach JNI thread");
        let activity = unsafe { JObject::from_raw(ctx.context().cast()) };
        f(&mut env, &activity)
    }

    pub fn status() -> MicPermission {
        with_activity(|env, activity| {
            let permission: JObject = env.new_string(RECORD_AUDIO).unwrap().into();
            // Activity.checkSelfPermission (API 23+)
            let result = env
                .call_method(
                    activity,
                    "checkSelfPermission",
                    "(Ljava/lang/String;)I",
                    &[JValue::Object(&permission)],
                )
                .and_then(|v| v.i())
                .unwrap_or(-1);
            if result == PERMISSION_GRANTED {
                MicPermission::Granted
            } else {
                MicPermission::NotDetermined
            }
        })
    }

    fn request() {
        with_activity(|env, activity| {
            let permission = env.new_string(RECORD_AUDIO).unwrap();
            let array: JObject = env
                .new_object_array(1, "java/lang/String", &permission)
                .unwrap()
                .into();
            // Activity.requestPermissions (API 23+)
            let _ = env.call_method(
                activity,
                "requestPermissions",
                "([Ljava/lang/String;I)V",
                &[JValue::Object(&array), JValue::Int(42)],
            );
        });
    }

    pub async fn ensure() -> bool {
        if status() == MicPermission::Granted {
            return true;
        }
        request();
        // GameActivity doesn't forward onRequestPermissionsResult into Rust,
        // so poll until the system dialog is answered (~30 s timeout).
        for _ in 0..150 {
            Delay::new(Duration::from_millis(200)).await;
            if status() == MicPermission::Granted {
                return true;
            }
        }
        false
    }
}

// -------------------------------------------------------------------- iOS --
#[cfg(target_os = "ios")]
mod imp {
    use super::MicPermission;
    use block2::RcBlock;
    use futures_channel::oneshot;
    use objc2::runtime::Bool;
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
    use std::sync::Mutex;

    pub fn status() -> MicPermission {
        // extern static; `unsafe` may be unnecessary depending on objc2 version
        let media_type = unsafe { &AVMediaTypeAudio };
        match unsafe { AVCaptureDevice::authorizationStatusForMediaType(media_type) } {
            AVAuthorizationStatus::Authorized => MicPermission::Granted,
            AVAuthorizationStatus::NotDetermined => MicPermission::NotDetermined,
            _ => MicPermission::Denied,
        }
    }

    pub async fn ensure() -> bool {
        if status() == MicPermission::Granted {
            return true;
        }
        let (tx, rx) = oneshot::channel::<bool>();
        let tx = Mutex::new(Some(tx));
        let handler = RcBlock::new(move |granted: Bool| {
            if let Some(tx) = tx.lock().unwrap().take() {
                let _ = tx.send(granted.as_bool());
            }
        });
        let media_type = unsafe { &AVMediaTypeAudio };
        // Shows the system prompt the first time; if the user already decided,
        // the handler fires immediately with the stored status (no dialog).
        unsafe {
            AVCaptureDevice::requestAccessForMediaType_completionHandler(media_type, &handler)
        };
        rx.await.unwrap_or(false)
    }
}

// ------------------------------------------------------- other platforms --
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod imp {
    use super::MicPermission;
    pub fn status() -> MicPermission { MicPermission::Granted }
    pub async fn ensure() -> bool { true }
}
