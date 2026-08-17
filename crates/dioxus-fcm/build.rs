use std::{fs, path::PathBuf};

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    if target_os == "ios" {
        stage_ios_google_service_plist();
        return;
    }

    if target_os != "android" {
        return;
    }

    // Reuse wry's env vars — dx sets these during the Android build.
    println!("cargo:rerun-if-env-changed=WRY_ANDROID_PACKAGE");
    println!("cargo:rerun-if-env-changed=WRY_ANDROID_KOTLIN_FILES_OUT_DIR");

    // Generate the JNI-exported symbols for `FCM_ANDROID_PACKAGE` (what dx set for
    // the last run through `rustc-env`) or fall back to dx's wry default. This must
    // run even when WRY_ANDROID_KOTLIN_FILES_OUT_DIR is absent (e.g. IDE
    // `cargo check --target android`), so android.rs's `include!` always resolves.
    let fallback_package = std::env::var("FCM_ANDROID_PACKAGE")
        .ok()
        .or_else(|| std::env::var("WRY_ANDROID_PACKAGE").ok())
        .unwrap_or_else(|| "dev.dioxus.main".to_string());
    emit_jni_symbols(&fallback_package);

    let Ok(kotlin_out_dir) = std::env::var("WRY_ANDROID_KOTLIN_FILES_OUT_DIR") else {
        // Not building through dx's android pipeline; nothing to generate.
        return;
    };

    let kotlin_out_dir = PathBuf::from(&kotlin_out_dir)
        .canonicalize()
        .unwrap_or_else(|_| panic!("Failed to canonicalize {kotlin_out_dir}"));

    // ---- Derive the Android app module root from the kotlin out dir ----
    // WRY_ANDROID_KOTLIN_FILES_OUT_DIR typically looks like:
    //   .../<app_module>/app/src/main/kotlin/<package/path>
    // Walk up until we find the "src" directory, then the app module is its parent.
    let app_module = {
        let mut dir = kotlin_out_dir.as_path();
        loop {
            match dir.parent() {
                Some(parent) if dir.file_name().map(|n| n == "src").unwrap_or(false) => {
                    break parent.to_path_buf();
                }
                Some(parent) => dir = parent,
                None => panic!(
                    "Could not locate app module root from {}",
                    kotlin_out_dir.display()
                ),
            }
        }
    };

    // ---- Resolve the Android application identifier (package name) ----
    // dx hardcodes `WRY_ANDROID_PACKAGE = dev.dioxus.main` (wry's internal
    // package), which does NOT match the app's real identifier from
    // Dioxus.toml `[bundle] identifier`. The generated app `build.gradle.kts`
    // carries the *actual* identifier (`namespace` / `applicationId`), so we
    // parse it from there. Fall back gracefully for non-dx/odd builds.
    let package = android_identifier_from_gradle(&app_module)
        .or_else(|| std::env::var("WRY_ANDROID_PACKAGE").ok())
        .unwrap_or_else(|| {
            println!(
                "cargo:warning=could not determine Android package identifier; \
                 defaulting to 'dev.dioxus.main'"
            );
            "dev.dioxus.main".to_string()
        });

    if package.is_empty() {
        println!(
            "cargo:warning=empty Android package identifier; FcmService will not be generated"
        );
        return;
    }

    // Make the resolved package observable to the runtime crate (android.rs), so
    // JNI lookups agree with the generated Kotlin class.
    println!("cargo:rustc-env=FCM_ANDROID_PACKAGE={package}");

    // Regenerate the JNI symbols with the authoritative package (overrides the
    // fallback emitted above).
    emit_jni_symbols(&package);

    // ---- Where the generated Kotlin files go ----
    // dx pins `WRY_ANDROID_KOTLIN_FILES_OUT_DIR` to .../kotlin/dev/dioxus/main
    // (wry's hardcoded package), which does NOT match the app's real identifier.
    // Since our generated class declares `package <real-identifier>`, its source
    // must live in <app>/src/main/kotlin/<real-identifier path> so the package
    // matches the directory (a gradle/kotlinc requirement). Override the out dir
    // with one derived from the resolved package.
    let kotlin_out_dir = app_module
        .join("src")
        .join("main")
        .join("kotlin")
        .join(package.replace('.', "/"));
    // ---- Copy google-services.json into the app module ----
    // The Google Services Gradle plugin (applied below) requires this file in the
    // app module. dx runs build scripts with cwd = CARGO_MANIFEST_DIR (the crate),
    // not the app, and does not expose the app root via env. dx lays the generated
    // gradle tree out under <app-root>/target/dx/<name>/<mode>/android/app/app, so
    // we can locate the app root by walking up until a `target` directory.
    let app_root = {
        let mut dir = Some(app_module.as_path());
        let mut found = None;
        while let Some(d) = dir {
            if d.join("target").is_dir() {
                found = Some(d.to_path_buf());
                break;
            }
            dir = d.parent();
        }
        found
    };

    let gs_src = app_root
        .as_ref()
        .map(|r| r.join("android/google-services.json"));

    if let Some(gs_src) = gs_src {
        if gs_src.exists() {
            let gs_dst = app_module.join("google-services.json");
            if let Err(e) = fs::copy(&gs_src, &gs_dst) {
                println!("cargo:warning=failed to copy google-services.json: {e}");
            } else {
                println!("cargo:rerun-if-changed={}", gs_src.display());
            }
        } else {
            println!(
                "cargo:warning=google-services.json not found at {}",
                gs_src.display()
            );
        }
    } else {
        println!("cargo:warning=could not locate app root; google-services.json not copied");
    }

    // ---- Apply the google-services Gradle plugin (auto-init from google-services.json) ----
    // AndroidArtifactMetadata injects `implementation(...)` deps but NOT the
    // `plugins {}` block or the buildscript classpath, so we wire those two here.
    // No Firebase values are hardcoded — the plugin reads everything from
    // google-services.json at build time.
    apply_google_services_plugin(&app_module);

    // Call the new function here:
    //enable_build_config(&app_module);

    // -------------------
    // ---- Generate Kotlin files from templates ----
    let template_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("src/android/kotlin");
    println!("cargo:rerun-if-changed={}", template_dir.display());

    for entry in fs::read_dir(&template_dir).expect("failed to read kotlin template dir") {
        let file = entry.unwrap();

        let content = fs::read_to_string(file.path())
            .expect("failed to read kotlin template")
            .replace("{{package}}", &package)
            .replace("{{package-unescaped}}", &package.replace('`', ""));

        let out = format!(
            "/* THIS FILE IS AUTO-GENERATED BY dioxus-fcm. DO NOT MODIFY!! */\n\n{content}"
        );

        let out_path = kotlin_out_dir.join(file.file_name());
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).expect("failed to create kotlin output dir");
        }
        if fs::read_to_string(&out_path).map_or(true, |o| o != out) {
            fs::write(&out_path, &out).expect("failed to write generated kotlin");
        }
        println!("cargo:rerun-if-changed={}", out_path.display());
    }

    // Although the app declares the FcmService in its own custom manifest, the
    // bundled dx CLI (0.8.0-alpha.1) ignores `[android] custom_manifest` and
    // writes its own generated manifest. To guarantee the class is registered,
    // inject the <service> element into the generated app manifest ourselves.
    ensure_fcm_service_in_manifest(&app_module, &package);
}

/// Emit the JNI-exported native-callback symbols into `OUT_DIR/generated_native.rs`.
/// JVM resolves `external` native methods by symbol name, so exporting the
/// `Java_<pkg>_FcmService_nativeOnXxx` symbols guarantees Firebase-triggered
/// callbacks (onNewToken etc.) bind even when they fire on a background thread
/// before the crate lazily calls `register_native_methods`.
fn emit_jni_symbols(package: &str) {
    let jni_class = format!("{}_FcmService", package.replace('.', "_"));
    let native_src = format!(
        "// AUTO-GENERATED by dioxus-fcm build.rs. DO NOT MODIFY.\n\
         #[unsafe(no_mangle)]\n\
         pub extern \"system\" fn Java_{jni_class}_nativeOnToken(\n             env: jni::JNIEnv, class: jni::objects::JClass, token: jni::objects::JString,\n         ) {{ crate::android::fcm_native_on_token(env, class, token) }}\n\
         #[unsafe(no_mangle)]\n\
         pub extern \"system\" fn Java_{jni_class}_nativeOnError(\n             env: jni::JNIEnv, class: jni::objects::JClass, msg: jni::objects::JString,\n         ) {{ crate::android::fcm_native_on_error(env, class, msg) }}\n\
         #[unsafe(no_mangle)]\n\
         pub extern \"system\" fn Java_{jni_class}_nativeOnPermissionResult(\n             _env: jni::JNIEnv, _class: jni::objects::JClass, granted: jni::sys::jboolean,\n         ) {{ crate::android::fcm_native_on_permission_result(_env, _class, granted) }}\n"
    );
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let native_out = out_dir.join("generated_native.rs");
    fs::create_dir_all(&out_dir).ok();
    if fs::read_to_string(&native_out).map_or(true, |o| o != native_src) {
        fs::write(&native_out, &native_src).expect("failed to write generated_native.rs");
    }
    println!("cargo:rerun-if-changed={}", native_out.display());
}

/// Parse the Android application identifier (package name) from the generated
/// app `build.gradle.kts`:
///
/// ```kotlin
/// android { namespace="org.taalbubbl.dev" ... }
///
/// defaultConfig { applicationId = "org.taalbubbl.dev" ... }
/// ```
///
/// dx populates this file from the project's Dioxus.toml `[bundle] identifier`
/// (via `bundle_identifier()`), so it is the authoritative source — unlike the
/// hardcoded `WRY_ANDROID_PACKAGE=dev.dioxus.main` env var that dx also sets for
/// wry's internal use.
fn android_identifier_from_gradle(app_module: &std::path::Path) -> Option<String> {
    let app_gradle = app_module.join("build.gradle.kts");
    let contents = fs::read_to_string(&app_gradle).ok()?;

    let candidate = contents
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            // namespace="x" or namespace = "x"
            let after_ns = line.strip_prefix("namespace")?;
            let q = after_ns.find('"')? + 1;
            let rest = &after_ns[q..];
            let end = rest.find('"')?;
            Some(rest[..end].to_string())
        })
        .find(|id| !id.is_empty());

    if candidate.is_none() {
        println!(
            "cargo:warning=no namespace found in {}; FcmService will use WRY_ANDROID_PACKAGE",
            app_gradle.display()
        );
    }
    candidate
}

/// Ensure the FcmService `<service>` entry appears in the generated app
/// AndroidManifest.xml. dx rewrites this file on every build, so we add the
/// element idempotently right after the `<application ...>` opening tag.
///
/// Failing to register the class means Firebase never delivers to it, and the
/// manifest merger silently drops an unresolved class.
fn ensure_fcm_service_in_manifest(app_module: &std::path::Path, package: &str) {
    let manifest_path = app_module
        .join("src")
        .join("main")
        .join("AndroidManifest.xml");
    let Ok(mut manifest) = fs::read_to_string(&manifest_path) else {
        println!(
            "cargo:warning=no AndroidManifest.xml at {}; FcmService not registered",
            manifest_path.display()
        );
        return;
    };

    let service_name = format!("{package}.FcmService");
    if manifest.contains(&service_name) {
        return;
    }

    let service_xml = format!(
        "\n        <service\n            android:name=\"{service_name}\"\n            android:exported=\"false\"\n        >\n            <intent-filter>\n                <action android:name=\"com.google.firebase.MESSAGING_EVENT\" />\n            </intent-filter>\n        </service>\n"
    );

    // Insert after the <application ...> opening tag (first ``<application`` line).
    if let Some(idx) = manifest.find("<application") {
        let insert_at = manifest[idx..]
            .find('>')
            .map(|off| idx + off + 1)
            .unwrap_or(manifest.len());
        manifest.insert_str(insert_at, &service_xml);
        if let Err(e) = fs::write(&manifest_path, &manifest) {
            println!("cargo:warning=failed to write AndroidManifest.xml for FcmService: {e}");
        }
        println!("cargo:rerun-if-changed={}", manifest_path.display());
    } else {
        println!(
            "cargo:warning=no <application> tag in AndroidManifest.xml; FcmService service not injected"
        );
    }
}

/// Inject the `com.google.gms.google-services` plugin into the generated Gradle
/// project. Idempotent: safe to run on every build (dx regenerates these files).
///
/// - project `build.gradle.kts`: add the plugin classpath to `buildscript { dependencies }`
/// - app `build.gradle.kts`: apply the plugin in the top-level `plugins { }` block
fn apply_google_services_plugin(app_module: &std::path::Path) {
    const GS_VERSION: &str = "4.4.2";

    // app_module = .../android/app/app
    // project root (has the buildscript) = .../android/app
    let project_root = app_module
        .parent()
        .expect("app module has no parent (project root)")
        .to_path_buf();

    let app_gradle = app_module.join("build.gradle.kts");
    let root_gradle = project_root.join("build.gradle.kts");

    // 1. Add the plugin classpath to the project buildscript.
    match fs::read_to_string(&root_gradle) {
        Ok(mut contents) => {
            if !contents.contains("com.google.gms:google-services") {
                let needle = "classpath(\"org.jetbrains.kotlin:kotlin-gradle-plugin";
                if let Some(pos) = contents.find(needle) {
                    let insert = format!(
                        "classpath(\"com.google.gms:google-services:{GS_VERSION}\")\n        "
                    );
                    contents.insert_str(pos, &insert);
                    fs::write(&root_gradle, contents)
                        .expect("failed to write project build.gradle.kts");
                } else {
                    println!(
                        "cargo:warning=could not find kotlin-gradle-plugin classpath in {}; \
                         google-services classpath not injected",
                        root_gradle.display()
                    );
                }
            }
        }
        Err(e) => println!(
            "cargo:warning=could not read {}: {e}",
            root_gradle.display()
        ),
    }

    // 2. Apply the plugin in the app module's plugins block.
    match fs::read_to_string(&app_gradle) {
        Ok(mut contents) => {
            if !contents.contains("com.google.gms.google-services") {
                let needle = "id(\"org.jetbrains.kotlin.android\")";
                if let Some(pos) = contents.find(needle) {
                    let end = pos + needle.len();
                    let insert = "\n    id(\"com.google.gms.google-services\")";
                    contents.insert_str(end, insert);
                    fs::write(&app_gradle, contents).expect("failed to write app build.gradle.kts");
                } else {
                    println!(
                        "cargo:warning=could not find kotlin.android plugin id in {}; \
                         google-services plugin not applied",
                        app_gradle.display()
                    );
                }
            }
        }
        Err(e) => println!("cargo:warning=could not read {}: {e}", app_gradle.display()),
    }
}
// Add this call to your main() function:
// apply_google_services_plugin(&app_module);
// enable_build_config(&app_module);

fn enable_build_config(app_module: &std::path::Path) {
    let app_gradle = app_module.join("build.gradle.kts");

    match fs::read_to_string(&app_gradle) {
        Ok(mut contents) => {
            // Check if buildConfig is already enabled to avoid duplicate injections
            if !contents.contains("buildConfig = true") {
                // We look for the 'android {' block to insert our configuration
                let needle = "android {";
                if let Some(pos) = contents.find(needle) {
                    let end = pos + needle.len();
                    let insert = "\n    buildFeatures {\n        buildConfig = true\n    }";

                    contents.insert_str(end, insert);
                    fs::write(&app_gradle, contents)
                        .expect("failed to write app build.gradle.kts (buildConfig)");
                    println!("cargo:warning=Successfully injected buildConfig = true");
                } else {
                    println!(
                        "cargo:warning=Could not find 'android {{' block in {}",
                        app_gradle.display()
                    );
                }
            }
        }
        Err(e) => println!("cargo:warning=Could not read {}: {e}", app_gradle.display()),
    }
}

/// Stage `Sources/Resources/GoogleService-Info.plist` (gitignored — see `.gitignore`)
/// for the Swift package build. Mirrors the Android side's `google-services.json` copy:
/// the *real* file lives in the consuming app's source tree (`<app>/ios/GoogleService-Info.plist`),
/// never inside this crate, so the crate stays reusable and no Firebase secrets end up
/// tracked in dioxus-fcm's own git history.
///
/// Falls back to the tracked `GoogleService-Info.example.plist` placeholder when the app
/// hasn't provided one yet, so the Swift package always has *something* to bundle (SwiftPM
/// requires the declared resource to exist) and `FcmPlugin` can detect "not configured yet"
/// safely instead of failing to build.
fn stage_ios_google_service_plist() {
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    let plugin_dir = manifest_dir.join("src/ios/plugin");
    let dest = plugin_dir.join("Sources/Resources/GoogleService-Info.plist");
    let example = plugin_dir.join("GoogleService-Info.example.plist");

    let src = find_app_google_service_plist(&manifest_dir).unwrap_or_else(|| {
        println!(
            "cargo:warning=<app>/ios/GoogleService-Info.plist not found — using the \
             dioxus-fcm placeholder, so Firebase push notifications won't work until you add \
             one (see crates/dioxus-fcm/README.MD)"
        );
        example
    });

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).ok();
    }
    if let Err(e) = fs::copy(&src, &dest) {
        println!(
            "cargo:warning=failed to copy {} to {}: {e}",
            src.display(),
            dest.display()
        );
    }
    println!("cargo:rerun-if-changed={}", src.display());
}

/// Walk up from this crate's manifest dir to the Cargo workspace root (the first ancestor
/// whose `Cargo.toml` has a `[workspace]` table), then look for `ios/GoogleService-Info.plist`
/// either at the workspace root itself or in one of its immediate member directories —
/// whichever consuming app has one.
fn find_app_google_service_plist(manifest_dir: &std::path::Path) -> Option<PathBuf> {
    let mut dir = manifest_dir.parent();
    while let Some(d) = dir {
        let cargo_toml = d.join("Cargo.toml");
        if let Ok(contents) = fs::read_to_string(&cargo_toml) {
            if contents.lines().any(|l| l.trim() == "[workspace]") {
                let direct = d.join("ios/GoogleService-Info.plist");
                if direct.is_file() {
                    return Some(direct);
                }
                for entry in fs::read_dir(d).ok()?.flatten() {
                    let candidate = entry.path().join("ios/GoogleService-Info.plist");
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
                return None;
            }
        }
        dir = d.parent();
    }
    None
}
