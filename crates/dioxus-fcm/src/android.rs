use jni::JNIEnv;
use jni::objects::{GlobalRef, JClass, JObject, JString, JThrowable, JValue};
use parking_lot::Mutex;

use std::time::Duration;
use tokio::sync::oneshot;

// Mutex::new is const — no once_cell needed
static PENDING_TOKEN: Mutex<Option<oneshot::Sender<Result<String, String>>>> = Mutex::new(None);
static PENDING_PERMISSION: Mutex<Option<oneshot::Sender<bool>>> = Mutex::new(None);

/// Package baked into the crate by dioxus-fcm's build.rs, derived from the
/// app's Dioxus.toml `[bundle] identifier` (via the generated `build.gradle.kts`
/// `namespace`/`applicationId`). This is the authoritative class package that
/// matches the generated `FcmService.kt`.
///
/// Do NOT rely on `option_env!("WRY_ANDROID_PACKAGE")`: dx hardcodes that to
/// `dev.dioxus.main` for wry's internal package, which almost never matches the
/// app's real identifier.
const ANDROID_PACKAGE: &str = match option_env!("FCM_ANDROID_PACKAGE") {
    Some(p) => p,
    // Compiled outside dx's Android pipeline (e.g. standalone `cargo check --target`
    // for IDE analysis): build.rs never resolved the identifier, so we have no
    // authoritative package. Use dx's wry default; a real build always overrides.
    None => "dev.dioxus.main",
};

/// Cache the FcmService `java.lang.Class` (as a GlobalRef) once it has been
/// resolved through the app ClassLoader. Subsequent JNI calls use the cached
/// class handle directly, which sidesteps `FindClass` — the call that fails on
/// non-main / attached threads with a spurious `ClassNotFoundException`.
static FCM_CLASS: Mutex<Option<GlobalRef>> = Mutex::new(None);

fn fcm_class_path() -> String {
    // "org.taalbubbl.dev" -> "org/taalbubbl/dev/FcmService"
    format!("{}/FcmService", ANDROID_PACKAGE.replace('.', "/"))
}

fn fcm_class_name() -> String {
    // Dotted class name for ClassLoader.loadClass — "org.taalbubbl.dev.FcmService"
    format!("{}.FcmService", ANDROID_PACKAGE)
}

/// Resolve and cache the FcmService class handle via the app's ClassLoader.
/// On Android, `FindClass` on an attached thread only sees system classes, so
/// we load through `context.getClassLoader().loadClass(...)` and keep a global
/// ref. Returns the cached class handle on success.
fn fcm_jclass(env: &mut JNIEnv) -> Option<GlobalRef> {
    // Fast path: already resolved.
    if let Some(cached) = FCM_CLASS.lock().clone() {
        return Some(cached);
    }

    let ctx = ndk_context::android_context();
    let context = unsafe { JObject::from_raw(ctx.context().cast()) };

    // context.getClassLoader()
    let class_loader =
        match env.call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[]) {
            Ok(v) => match v.l() {
                Ok(o) => o,
                Err(e) => {
                    tracing::error!("getClassLoader returned non-object: {e:?}");
                    return None;
                }
            },
            Err(e) => {
                let detail = describe_pending_exception(env);
                tracing::error!("getClassLoader failed: {e:?} | java: {detail}");
                return None;
            }
        };

    // classLoader.loadClass("org.taalbubbl.dev.FcmService")  (dots, not slashes)
    let Ok(class_name) = env.new_string(fcm_class_name()) else {
        tracing::error!("new_string failed for FcmService class name");
        return None;
    };

    let clazz_obj = match env.call_method(
        &class_loader,
        "loadClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        &[JValue::Object(&class_name)],
    ) {
        Ok(v) => match v.l() {
            Ok(o) => o,
            Err(e) => {
                tracing::error!("loadClass returned non-object: {e:?}");
                return None;
            }
        },
        Err(e) => {
            let detail = describe_pending_exception(env);
            tracing::error!(
                "loadClass failed for {}: {e:?} | java: {detail}",
                fcm_class_name()
            );
            return None;
        }
    };

    match env.new_global_ref(&clazz_obj) {
        Ok(global) => {
            // Once registered, nativeOnToken / nativeOnError / nativeOnPermissionResult
            // are bound to these Rust fns. Do this before anything can fire them.
            let class = unsafe { JClass::from_raw(clazz_obj.into_raw()) };
            if let Err(e) = env.register_native_methods(class, &native_callback_methods()) {
                let detail = describe_pending_exception(env);
                tracing::warn!("register_native_methods failed: {e:?} | java: {detail}");
            }
            *FCM_CLASS.lock() = Some(global.clone());
            Some(global)
        }
        Err(e) => {
            tracing::error!("new_global_ref for FcmService class failed: {e:?}");
            None
        }
    }
}

/// The native-callback descriptors, bound to the FcmService class.
fn native_callback_methods() -> [jni::NativeMethod; 3] {
    [
        jni::NativeMethod {
            name: "nativeOnToken".into(),
            sig: "(Ljava/lang/String;)V".into(),
            fn_ptr: fcm_native_on_token as *mut std::ffi::c_void,
        },
        jni::NativeMethod {
            name: "nativeOnError".into(),
            sig: "(Ljava/lang/String;)V".into(),
            fn_ptr: fcm_native_on_error as *mut std::ffi::c_void,
        },
        jni::NativeMethod {
            name: "nativeOnPermissionResult".into(),
            sig: "(Z)V".into(),
            fn_ptr: fcm_native_on_permission_result as *mut std::ffi::c_void,
        },
    ]
}

/// Resolve a JClass usable as a static-method receiver from the cached global
/// ref. Falls back to the older FindClass path only if the class was never
/// resolved through the ClassLoader.
fn get_fcm_class<'local>(env: &mut JNIEnv<'local>) -> Option<JClass<'local>> {
    if let Some(g) = fcm_jclass(env) {
        // &GlobalRef implements Desc<JClass<'static>>; JClass is repr(transparent)
        // over JObject. Re-wrap the JObject reference as a class handle.
        let raw = g.as_obj().as_raw();
        return Some(unsafe { jni::objects::JClass::from_raw(raw) });
    }
    // Legacy fallback: FindClass directly by name.
    env.find_class(fcm_class_path()).ok()
}

/// Incoming native-callback handlers. Inner implementations (no exported symbol):
/// the JVM-facing `Java_..._FcmService_nativeOnXxx` symbols are generated per-package
/// by build.rs (see `generated_native.rs`, included below) so they match the app's
/// resolved identifier and bind before any Firebase-triggered callback can fire.
///
/// JNI signature: `(JNIEnv*, jobject clazz, jstring token) -> void`.
pub extern "system" fn fcm_native_on_token(mut env: JNIEnv, _class: JClass, token: JString) {
    match env.get_string(&token) {
        Ok(s) => {
            let token: String = s.into();
            // take() first, drop guard, then send (no lock held during send)
            let tx = PENDING_TOKEN.lock().take();
            if let Some(tx) = tx {
                let _ = tx.send(Ok(token.clone())); // Err = receiver timed out, fine
            }
            crate::on_native_token(token); // broadcast for refresh events
        }
        Err(e) => tracing::error!("nativeOnToken get_string failed: {e:?}"),
    }
}

pub extern "system" fn fcm_native_on_error(mut env: JNIEnv, _class: JClass, msg: JString) {
    if let Ok(s) = env.get_string(&msg) {
        let msg: String = s.into();
        if let Some(tx) = PENDING_TOKEN.lock().take() {
            let _ = tx.send(Err(msg.clone()));
        }
        crate::on_native_error(msg);
    }
}

pub extern "system" fn fcm_native_on_permission_result(
    _env: JNIEnv,
    _class: JClass,
    granted: jni::sys::jboolean,
) {
    let granted = granted != 0;
    tracing::info!("🔔 Notification permission granted: {granted}");
    let tx = PENDING_PERMISSION.lock().take();
    if let Some(tx) = tx {
        let _ = tx.send(granted);
    }
}

// Generate the JNI-exported `Java_<underscored-package>_FcmService_nativeOnXxx`
// symbols from build.rs so the JVM can resolve them by name without any prior
// `RegisterNatives` call (Firebase may invoke onNewToken on a background thread
// before the crate ever resolves the class).
#[cfg(target_os = "android")]
include!(concat!(env!("OUT_DIR"), "/generated_native.rs"));

/// Ask the Kotlin side to fetch the FCM token.
pub async fn request_token() -> Result<String, String> {
    let (tx, rx) = oneshot::channel();
    *PENDING_TOKEN.lock() = Some(tx);

    fire_request_token();

    match tokio::time::timeout(Duration::from_secs(15), rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err("FCM token request was cancelled by the native layer".to_string()),
        Err(_) => {
            *PENDING_TOKEN.lock() = None;
            Err("request_token timed out".to_string())
        }
    }
}

fn fire_request_token() {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.unwrap();
    let mut env = vm.attach_current_thread().unwrap();
    let context = unsafe { JObject::from_raw(ctx.context().cast()) };

    let Some(class) = get_fcm_class(&mut env) else {
        let msg = "requestToken failed: could not resolve FcmService class".to_string();
        tracing::error!("{msg}");
        if let Some(tx) = PENDING_TOKEN.lock().take() {
            let _ = tx.send(Err(msg));
        }
        return;
    };

    if let Err(e) = env.call_static_method(
        class,
        "requestToken",
        "(Landroid/content/Context;)V",
        &[(&context).into()],
    ) {
        let detail = describe_pending_exception(&mut env);
        let msg = format!("requestToken failed: {e:?} | java: {detail}");
        tracing::error!("{msg}");
        if let Some(tx) = PENDING_TOKEN.lock().take() {
            let _ = tx.send(Err(msg));
        }
    }
}

//// Probe: ask the Kotlin side if it's reachable. Should return "inshallah".
pub fn test_interface() -> Option<String> {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.unwrap();
    let mut env = vm.attach_current_thread().unwrap();

    tracing::debug!("probing kotlin reachability");

    // Resolve through the app ClassLoader (never FindClass on an attached
    // thread) and invoke the static method on the class handle.
    let class = get_fcm_class(&mut env)?;
    match env.call_static_method(class, "kotlinAvailable", "()Ljava/lang/String;", &[]) {
        Ok(val) => read_string(&mut env, val),
        Err(e) => {
            let detail = describe_pending_exception(&mut env);
            tracing::error!("kotlinAvailable failed: {e:?} | java: {detail}");
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

    let Some(class) = get_fcm_class(&mut env) else {
        tracing::error!("init failed: could not resolve FcmService class");
        return;
    };

    match env.call_static_method(
        class,
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
    *PENDING_PERMISSION.lock() = Some(tx);

    if !fire_permission_request() {
        *PENDING_PERMISSION.lock() = None;
        return false;
    }

    // Generous timeout — a human is staring at a dialog
    match tokio::time::timeout(Duration::from_secs(120), rx).await {
        Ok(Ok(granted)) => granted,
        _ => {
            *PENDING_PERMISSION.lock() = None;
            false
        }
    }
}

fn fire_permission_request() -> bool {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.unwrap();
    let mut env = vm.attach_current_thread().unwrap();
    let activity = unsafe { JObject::from_raw(ctx.context().cast()) };

    let Some(class) = get_fcm_class(&mut env) else {
        tracing::error!("requestNotificationPermission failed: could not resolve FcmService class");
        return false;
    };

    match env.call_static_method(
        class,
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

    let Some(class) = get_fcm_class(&mut env) else {
        tracing::error!("notificationsEnabled failed: could not resolve FcmService class");
        return false;
    };

    env.call_static_method(
        class,
        "notificationsEnabled",
        "(Landroid/content/Context;)Z",
        &[(&context).into()],
    )
    .and_then(|v| v.z())
    .unwrap_or(false)
}
