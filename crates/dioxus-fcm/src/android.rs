use jni::JNIEnv;
use jni::objects::{JClass, JObject, JString, JThrowable, JValue};

use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::oneshot;

// Mutex::new is const — no once_cell needed
static PENDING_TOKEN: Mutex<Option<oneshot::Sender<String>>> = Mutex::new(None);
static PENDING_PERMISSION: Mutex<Option<oneshot::Sender<bool>>> = Mutex::new(None);

/// Package baked in at build time from WRY_ANDROID_PACKAGE.
/// Falls back for non-dx builds / IDE analysis.
const ANDROID_PACKAGE: &str = match option_env!("FCM_ANDROID_PACKAGE") {
    Some(p) => p,
    None => "dev.dioxus.main",
};

fn fcm_class_path() -> String {
    // "dev.dioxus.main" -> "dev/dioxus/main/FcmService"
    format!("{}/FcmService", ANDROID_PACKAGE.replace('.', "/"))
}
/// Called FROM Kotlin when a token is obtained or refreshed.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_dioxus_main_FcmService_nativeOnToken(
    mut env: JNIEnv,
    _class: JClass,
    token: JString,
) {
    match env.get_string(&token) {
        Ok(s) => {
            let token: String = s.into();
            // take() first, drop guard, then send (edition-safe, no lock held during send)
            let tx = PENDING_TOKEN.lock().unwrap().take();
            if let Some(tx) = tx {
                let _ = tx.send(token.clone()); // Err = receiver timed out, fine
            }
            crate::on_native_token(token); // keep your broadcast for refresh events
        }
        Err(e) => tracing::error!("nativeOnToken get_string failed: {e:?}"),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_dioxus_main_FcmService_nativeOnError(
    mut env: JNIEnv,
    _class: JClass,
    msg: JString,
) {
    if let Ok(s) = env.get_string(&msg) {
        let msg: String = s.into();
        // Dropping the sender wakes the awaiter with RecvError → returns None
        let _ = PENDING_TOKEN.lock().unwrap().take();
        crate::on_native_error(msg);
    }
}

/// Ask the Kotlin side to fetch the FCM token.
pub async fn request_token() -> Option<String> {
    let (tx, rx) = oneshot::channel();
    *PENDING_TOKEN.lock().unwrap() = Some(tx);

    fire_request_token(); // old body, returns ()

    match tokio::time::timeout(Duration::from_secs(15), rx).await {
        Ok(Ok(token)) => Some(token),
        Ok(Err(_)) => None, // nativeOnError dropped the sender
        Err(_) => {
            *PENDING_TOKEN.lock().unwrap() = None; // don't leak the slot
            tracing::error!("request_token timed out");
            None
        }
    }
}

fn fire_request_token() {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.unwrap();
    let mut env = vm.attach_current_thread().unwrap();
    let context = unsafe { JObject::from_raw(ctx.context().cast()) };

    if let Err(e) = env.call_static_method(
        fcm_class_path(),
        "requestToken",
        "(Landroid/content/Context;)V",
        &[(&context).into()],
    ) {
        let detail = describe_pending_exception(&mut env);
        tracing::error!("requestToken failed: {e:?} | java: {detail}");
        *PENDING_TOKEN.lock().unwrap() = None; // fail fast, wake the awaiter
    }
}

//// Probe: ask the Kotlin side if it's reachable. Should return "inshallah".
pub fn kotlin_available() -> Option<String> {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.unwrap();
    let mut env = vm.attach_current_thread().unwrap();

    tracing::debug!("probing kotlin reachability");

    // Try the simple path first: JNI FindClass via call_static_method.
    if let Some(s) = try_call(&mut env) {
        return Some(s);
    }

    // If that failed, try loading the class through the app's ClassLoader.
    // On Android, the JNI FindClass used on a non-main / attached thread often
    // only sees system classes, NOT your app's classes — which produces a
    // spurious ClassNotFoundException even when the class IS in the APK.
    tracing::warn!("direct call failed, retrying via app ClassLoader");
    try_call_via_classloader(&mut env)
}

/// Straight JNI call. Returns None on any failure (after logging the reason).
fn try_call(env: &mut JNIEnv) -> Option<String> {
    let result = env.call_static_method(
        &fcm_class_path(),
        "kotlinAvailable",
        "()Ljava/lang/String;",
        &[],
    );

    match result {
        Ok(val) => read_string(env, val),
        Err(e) => {
            let detail = describe_pending_exception(env);
            tracing::error!("direct kotlinAvailable failed: {e:?} | java: {detail}");
            None
        }
    }
}

/// Load com.example.TestApp.FcmService via the Android context's ClassLoader,
/// then invoke the static method reflectively-ish through JNI.
fn try_call_via_classloader(env: &mut JNIEnv) -> Option<String> {
    let ctx = ndk_context::android_context();
    let context = unsafe { JObject::from_raw(ctx.context().cast()) };

    // context.getClassLoader()
    let class_loader =
        match env.call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[]) {
            Ok(v) => match v.l() {
                Ok(o) => o,
                Err(_) => {
                    tracing::error!("getClassLoader returned non-object");
                    return None;
                }
            },
            Err(e) => {
                let detail = describe_pending_exception(env);
                tracing::error!("getClassLoader failed: {e:?} | java: {detail}");
                return None;
            }
        };

    // classLoader.loadClass("com.example.TestApp.FcmService")  (note: DOTS, not slashes)
    let dotted = format!("{}.FcmService", ANDROID_PACKAGE);
    let class_name = match env.new_string(&dotted) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("new_string failed: {e:?}");
            return None;
        }
    };

    let clazz_obj = match env.call_method(
        &class_loader,
        "loadClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        &[JValue::Object(&class_name)],
    ) {
        Ok(v) => match v.l() {
            Ok(o) => o,
            Err(_) => {
                tracing::error!("loadClass returned non-object");
                return None;
            }
        },
        Err(e) => {
            let detail = describe_pending_exception(env);
            tracing::error!("loadClass failed: {e:?} | java: {detail}");
            return None;
        }
    };

    // Now call the static method on the loaded jclass.
    let clazz: jni::objects::JClass = clazz_obj.into();
    match env.call_static_method(clazz, "kotlinAvailable", "()Ljava/lang/String;", &[]) {
        Ok(val) => read_string(env, val),
        Err(e) => {
            let detail = describe_pending_exception(env);
            tracing::error!("classloader kotlinAvailable failed: {e:?} | java: {detail}");
            None
        }
    }
}

/// Convert a returned JValue (expected String) into a Rust String.
fn read_string(env: &mut JNIEnv, val: jni::objects::JValueOwned) -> Option<String> {
    match val.l() {
        Ok(obj) => {
            let jstr: JString = obj.into();
            match env.get_string(&jstr) {
                Ok(s) => {
                    let s: String = s.into();
                    tracing::info!("Kotlin says: {s}"); // expect "inshallah"
                    Some(s)
                }
                Err(e) => {
                    tracing::error!("get_string failed: {e:?}");
                    None
                }
            }
        }
        Err(e) => {
            tracing::error!("return value not an object: {e:?}");
            None
        }
    }
}

/// Extract the pending Java exception's toString(), then clear it so ART
/// doesn't abort the VM. Safe to call even if nothing is pending.
fn describe_pending_exception(env: &mut JNIEnv) -> String {
    if !env.exception_check().unwrap_or(false) {
        return "<no pending exception>".to_string();
    }

    let throwable: JThrowable = match env.exception_occurred() {
        Ok(t) => t,
        Err(_) => {
            let _ = env.exception_clear();
            return "<could not obtain throwable>".to_string();
        }
    };
    let _ = env.exception_clear(); // MUST clear before making further JNI calls

    match env.call_method(&throwable, "toString", "()Ljava/lang/String;", &[]) {
        Ok(v) => match v.l() {
            Ok(obj) => {
                let jstr: JString = obj.into();
                env.get_string(&jstr)
                    .map(|s| s.into())
                    .unwrap_or_else(|_| "<toString unreadable>".to_string())
            }
            Err(_) => "<toString not object>".to_string(),
        },
        Err(_) => {
            let _ = env.exception_clear();
            "<toString threw>".to_string()
        }
    }
}
/// Initialize Firebase/App logic
pub fn init_fcm() {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.unwrap();
    let mut env = vm.attach_current_thread().unwrap();

    let context = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };

    tracing::debug!("calling FcmService.init");

    match env.call_static_method(
        &fcm_class_path(),
        "init",
        "(Landroid/content/Context;)V",
        &[(&context).into()],
    ) {
        Ok(_) => tracing::debug!("FcmService.init invoked successfully"),
        Err(e) => {
            let detail = describe_pending_exception(&mut env);
            tracing::error!("init failed: {e:?} | java: {detail}");
        }
    }
}

pub async fn request_notification_permission() -> bool {
    if notifications_enabled() {
        return true; // already granted, no dialog needed
    }

    let (tx, rx) = oneshot::channel();
    *PENDING_PERMISSION.lock().unwrap() = Some(tx);

    if !fire_permission_request() {
        *PENDING_PERMISSION.lock().unwrap() = None;
        return false;
    }

    // Generous timeout — a human is staring at a dialog
    match tokio::time::timeout(Duration::from_secs(120), rx).await {
        Ok(Ok(granted)) => granted,
        _ => {
            *PENDING_PERMISSION.lock().unwrap() = None;
            false
        }
    }
}

fn fire_permission_request() -> bool {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.unwrap();
    let mut env = vm.attach_current_thread().unwrap();
    let activity = unsafe { JObject::from_raw(ctx.context().cast()) };

    match env.call_static_method(
        fcm_class_path(), // ← was hardcoded "dev/dioxus/main/FcmService"
        "requestNotificationPermission",
        "(Landroid/app/Activity;)V",
        &[JValue::Object(&activity)],
    ) {
        Ok(_) => true,
        Err(e) => {
            let detail = describe_pending_exception(&mut env);
            tracing::error!("requestNotificationPermission failed: {e:?} | java: {detail}");
            false
        }
    }
}

/// Synchronous — safe to call anywhere, no dialog.
pub fn notifications_enabled() -> bool {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.unwrap();
    let mut env = vm.attach_current_thread().unwrap();
    let context = unsafe { JObject::from_raw(ctx.context().cast()) };

    env.call_static_method(
        fcm_class_path(),
        "notificationsEnabled",
        "(Landroid/content/Context;)Z",
        &[(&context).into()],
    )
    .and_then(|v| v.z())
    .unwrap_or(false)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_dioxus_main_FcmService_nativeOnPermissionResult(
    _env: JNIEnv,
    _class: JClass,
    granted: jni::sys::jboolean,
) {
    let granted = granted != 0;
    tracing::info!("🔔 Notification permission granted: {granted}");
    let tx = PENDING_PERMISSION.lock().unwrap().take();
    if let Some(tx) = tx {
        let _ = tx.send(granted);
    }
}
