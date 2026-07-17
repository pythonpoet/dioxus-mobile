use jni::JNIEnv;
use jni::objects::{JClass, JObject, JString, JThrowable, JValue};

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
/// JNI name must match: Java_com_yourapp_FcmService_nativeOnToken
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_dioxus_main_FcmServiceKt_nativeOnToken(
    mut env: JNIEnv,
    _class: JClass,
    token: JString,
) {
    tracing::info!("🔥 nativeOnToken CALLED");
    match env.get_string(&token) {
        Ok(s) => {
            let token: String = s.into();
            tracing::info!("🔥 FCM TOKEN: {token}");
            crate::on_native_token(token);
        }
        Err(e) => tracing::error!("nativeOnToken get_string failed: {e:?}"),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_dioxus_main_FcmServiceKt_nativeOnError(
    mut env: JNIEnv,
    _class: JClass,
    msg: JString,
) {
    if let Ok(s) = env.get_string(&msg) {
        crate::on_native_error(s.into());
    }
}

/// Ask the Kotlin side to fetch the FCM token.
pub fn request_token() {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.unwrap();
    let mut env = vm.attach_current_thread().unwrap();

    tracing::debug!("calling FcmService.requestToken");

    let context = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };
    match env.call_static_method(
        &fcm_class_path(),
        "requestToken",
        "(Landroid/content/Context;)V",
        &[(&context).into()],
    ) {
        Ok(_) => tracing::debug!("requestToken invoked successfully"),
        Err(e) => {
            let detail = describe_pending_exception(&mut env);
            tracing::error!("requestToken failed: {e:?} | java: {detail}");
        }
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
    let class_name = match env.new_string("com.example.TestApp.FcmService") {
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

/// Ask Kotlin to show the Notification Permission prompt
pub fn ask_notification_permission() {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.unwrap();
    let mut env = vm.attach_current_thread().unwrap();

    tracing::debug!("triggering Android Notification Permission");

    // "dev.dioxus.main" -> "dev/dioxus/main/MainActivity"
    let main_activity_path = format!("{}/MainActivity", ANDROID_PACKAGE.replace('.', "/"));

    match env.call_static_method(
        main_activity_path,
        "triggerNotificationPermission",
        "()V",
        &[],
    ) {
        Ok(_) => tracing::debug!("Permission prompt triggered successfully"),
        Err(e) => {
            let detail = describe_pending_exception(&mut env);
            tracing::error!("Permission trigger failed: {e:?} | java: {detail}");
        }
    }
}
