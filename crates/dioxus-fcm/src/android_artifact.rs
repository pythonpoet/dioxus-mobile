//! Contributes Firebase Gradle dependencies + the google-services plugin to the
//! host Dioxus Android build, via manganis' linker-symbol artifact mechanism.
//!
//! This replicates what `#[manganis::ffi("...")]` emits, EXCEPT we populate the
//! `gradle_dependencies` field (the macro hardcodes it to "").

#[cfg(target_os = "android")]
const _: () = {
    use manganis::android::AndroidArtifactMetadata;
    use manganis::android::macro_helpers::copy_bytes;
    use manganis::android::metadata::{AndroidMetadataBuffer, serialize_android_metadata};

    const __FCM_METADATA: AndroidArtifactMetadata = AndroidArtifactMetadata::new(
        "dioxus_fcm", // module name → :plugins:dioxus_fcm  (NOT a maven coord!)
        concat!(env!("CARGO_MANIFEST_DIR"), "/src/android"),
        concat!(
            "implementation(platform(\"com.google.firebase:firebase-bom:33.5.1\"))\n",
            "implementation(\"com.google.firebase:firebase-messaging\")",
        ),
    );

    // --- linker section (mirrors generate_link_section_inner) ---
    #[used]
    static __LINK_SECTION: &'static [u8] = {
        const __BUFFER: AndroidMetadataBuffer = serialize_android_metadata(&__FCM_METADATA);
        const __BYTES: &[u8] = __BUFFER.as_ref();
        const __LEN: usize = __BYTES.len();

        // Hash must be unique per artifact; any stable string works.
        #[unsafe(export_name = "__ASSETS__dioxusfcm00000001")]
        #[used]
        static __LINK_SECTION: [u8; __LEN] = copy_bytes(__BYTES);
        &__LINK_SECTION
    };
};
