//! Four-page test app for the dioxus-mobile workspace crates:
//! dioxus-fcm, dioxus-jwt, dioxus-recorder.

#![allow(non_snake_case)]

use dioxus::prelude::*;

mod views;
use views::{Fcm, Init, Jwt, Recorder};

/// Server URL used by native/mobile clients for server-function calls.
/// On a device, replace with the host's LAN IP, e.g. `http://192.168.1.10:8080`.
#[cfg(not(feature = "server"))]
const SERVER_URL: &str = "http://127.0.0.1:8080";

/// HS256 secret shared by the login server function and the auth-check decoder.
/// Override at runtime with the `JWT_SECRET` environment variable.
#[cfg(feature = "server")]
const JWT_SECRET: &str = "dev-only-secret-change-me";

/// Claims carried by our JWTs.
#[cfg(feature = "server")]
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct AuthClaims {
    sub: String,
    exp: u64,
}

/// Axum state exposing the JWT decoder to the auth-check route.
#[cfg(feature = "server")]
#[derive(Clone)]
struct AuthState {
    decoder: dioxus_jwt::Decoder<AuthClaims>,
}

pub fn App() -> Element {
    // 2. UI State Signals
    let mut is_recording = use_signal(|| false);
    let mut is_playing = use_signal(|| false);
    let mut mic_permission_state = use_signal(|| "Unknown".to_string());
    let mut file_info_str = use_signal(|| "No recording found".to_string());
    let mut notif_permission = use_signal(|| None::<bool>);
    let mut fcm_token = use_signal(|| None::<String>);

    // Initialize Firebase / the native notification bridge once at startup
    // (Android: FCM init; iOS: configures Firebase from GoogleService-Info.plist).
    use_effect(move || {
        dioxus_fcm::init_fcm();
    });

    // 3. Helper to dynamically update the recording file size metadata on the UI
    let mut refresh_file_metadata = move || {
        let path = get_recording_path();
        if path.exists() {
            if let Ok(metadata) = std::fs::metadata(&path) {
                let bytes = metadata.len();
                let kb = bytes as f64 / 1024.0;
                if kb > 1024.0 {
                    file_info_str.set(format!("{:.2} MB", kb / 1024.0));
                } else {
                    file_info_str.set(format!("{:.1} KB", kb));
                }
                return;
            }
        }
        file_info_str.set("No recording found".to_string());
    };

    // Run once on initialization to check for existing files
    use_effect(move || {
        refresh_file_metadata();
    });

    // 4. Central Multi-threaded Audio Manager Coroutine
    let audio_task = use_coroutine(move |mut rx: UnboundedReceiver<AudioCommand>| async move {
        let mut record_stream: Option<cpal::Stream> = None;
        let mut playback_stream: Option<cpal::Stream> = None;
        let mut recording_flag: Option<Arc<AtomicBool>> = None;
        let writer_mutex: Arc<Mutex<Option<WavWriter<BufWriter<File>>>>> = Arc::new(Mutex::new(None));

        while let Some(cmd) = rx.next().await {
            match cmd {
                // ==========================================
                // START RECORDING COMMAND
                // ==========================================
                AudioCommand::StartRecord => {
                    // Explicitly stop any running playback first
                    playback_stream = None;

                    let host = cpal::default_host();
                    let device = match host.default_input_device() {
                        Some(d) => d,
                        None => {
                            eprintln!("Error: No microphone device found.");
                            continue;
                        }
                    };

                    let config = cpal::StreamConfig {
                        channels: 1, // nnnoiseless operates on single-channel mono input
                        sample_rate: cpal::SampleRate(48000), // Native DSP sample rate
                        buffer_size: cpal::BufferSize::Default,
                    };

                    let spec = WavSpec {
                        channels: 1,
                        sample_rate: 48000,
                        bits_per_sample: 32,
                        sample_format: hound::SampleFormat::Float,
                    };

                    // Access secure sandboxed filepath safely
                    let file_path = get_recording_path();
                    let writer = match WavWriter::create(&file_path, spec) {
                        Ok(w) => w,
                        Err(e) => {
                            eprintln!("Failed to write file to storage: {}", e);
                            continue;
                        }
                    };
                    *writer_mutex.lock().unwrap() = Some(writer);

                    // Multi-thread Ring Buffer Setup (Decouples audio callback thread from blocking I/O)
                    let rb = SharedRb::<Heap<f32>>::new(8192);
                    let (mut prod, mut cons) = rb.split();

                    let writer_clone = Arc::clone(&writer_mutex);
                    let is_recording_flag = Arc::new(AtomicBool::new(true));
                    recording_flag = Some(Arc::clone(&is_recording_flag));

                    // Thread 1: DSP Processing & File I/O Loop
                    std::thread::spawn(move || {
                        let mut denoiser = DenoiseState::new();
                        let mut input_frame = [0.0f32; 480];
                        let mut output_frame = [0.0f32; 480];

                        while is_recording_flag.load(Ordering::Relaxed) || cons.occupied_len() >= 480 {
                            if cons.occupied_len() >= 480 {
                                cons.pop_slice(&mut input_frame);

                                // Perform live background noise stripping
                                denoiser.process_frame(&mut output_frame, &input_frame);

                                if let Ok(mut guard) = writer_clone.lock() {
                                    if let Some(w) = guard.as_mut() {
                                        for &sample in &output_frame {
                                            let _ = w.write_sample(sample);
                                        }
                                    }
                                }
                            } else {
                                // Minimize CPU overhead when waiting for microphone samples
                                std::thread::sleep(std::time::Duration::from_millis(5));
                            }
                        }

                        // Close and finalize the file headers securely
                        if let Ok(mut guard) = writer_clone.lock() {
                            if let Some(w) = guard.take() {
                                let _ = w.finalize();
                            }
                        }
                    });

                    // Thread 2: High-Priority Native OS Audio Callback
                    let new_stream = device.build_input_stream(
                        &config,
                        move |data: &[f32], _: &_| {
                            let _ = prod.push_slice(data);
                        },
                        |err| tragedies_happen(err),
                        None,
                    ).expect("Failed to initialize recording hardware");

                    new_stream.play().unwrap();
                    record_stream = Some(new_stream);
                }

                // ==========================================
                // STOP RECORDING COMMAND
                // ==========================================
                AudioCommand::StopRecord => {
                    record_stream = None;
                    if let Some(flag) = recording_flag.take() {
                        flag.store(false, Ordering::Relaxed);
                    }
                    // Tiny buffer delay to allow the background task to flush and write headers
                    std::thread::sleep(std::time::Duration::from_millis(150));
                }

/// Bare-bones login server function: fixed credentials, no database.
#[post("/api/login")]
pub async fn login(username: String, password: String) -> ServerFnResult<String> {
    if username == "admin" && password == "password" {
        let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| JWT_SECRET.to_string());
        let claims = AuthClaims {
            sub: username,
            exp: now_unix() + 86_400, // 24h
        };
        dioxus_jwt::issue_hs256(&secret, &claims)
            .ok_or_else(|| ServerFnError::new("failed to sign token"))
    } else {
        Err(ServerFnError::new("invalid credentials"))
    }
}

/// Server-side check: validate the bearer token against the decoder.
#[cfg(feature = "server")]
async fn auth_check(claims: dioxus_jwt::Claims<AuthClaims>) -> String {
    format!("authenticated as {}", claims.claims.sub)
}

/// Current unix time in seconds, matching the JWT `exp` claim format.
#[cfg(feature = "server")]
fn now_unix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Clone, Routable, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[rustfmt::skip]
enum Route {
    #[layout(Layout)]
        #[route("/")]
        Init {},

        #[route("/fcm")]
        Fcm {},

        #[route("/jwt")]
        Jwt {},

        #[route("/recorder")]
        Recorder {},
}

#[component]
fn Layout() -> Element {
    rsx! {
        nav {
            style: "display: flex; gap: 8px; flex-wrap: wrap; padding: 12px 16px; background: #0f172a; border-bottom: 1px solid rgba(255,255,255,0.08);",
            Link { to: Route::Init {}, style: "color: #93c5fd; text-decoration: none; padding: 6px 10px; border-radius: 8px;", "Init" }
            Link { to: Route::Fcm {}, style: "color: #93c5fd; text-decoration: none; padding: 6px 10px; border-radius: 8px;", "FCM" }
            Link { to: Route::Jwt {}, style: "color: #93c5fd; text-decoration: none; padding: 6px 10px; border-radius: 8px;", "JWT" }
            Link { to: Route::Recorder {}, style: "color: #93c5fd; text-decoration: none; padding: 6px 10px; border-radius: 8px;", "Recorder" }
        }
        Outlet::<Route> {}
    }
}

fn App() -> Element {
    let _auth = dioxus_jwt::init();
    rsx! { Router::<Route> {} }
}

fn main() {
    #[cfg(not(feature = "server"))]
    dioxus::fullstack::set_server_url(SERVER_URL);

    #[cfg(not(feature = "server"))]
    dioxus::launch(App);

    #[cfg(feature = "server")]
    dioxus::serve(|| async move {
        use dioxus::server::axum::routing::get;
        use dioxus::server::axum::Router;

        let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| JWT_SECRET.to_string());
        let decoder = dioxus_jwt::hs256_decoder::<AuthClaims>(&secret, Some(&["test-app"]));
        let auth_state = AuthState { decoder };

        let auth_router = Router::new()
            .route("/api/check", get(auth_check))
            .with_state(auth_state);

        Ok(dioxus::server::router(App).merge(auth_router))
    });
}
