use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use dioxus::prelude::*;
use futures_util::StreamExt;
use hound::{WavSpec, WavWriter};
use nnnoiseless::DenoiseState;
use ringbuf::{storage::Heap, traits::*, SharedRb};
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Commands routed to our background audio loop manager
enum AudioCommand {
    StartRecord,
    StopRecord,
    StartPlayback,
    StopPlayback,
}

fn main() {
    // 1. Initialize Android Logging (Redirects Rust panics and logs cleanly to Logcat)
    // #[cfg(target_os = "android")]
    // {
    //     android_logger::init_once(
    //         android_logger::Config::default()
    //             .with_tag("test-app")
    //             .with_max_level(log::LevelFilter::Debug),
    //     );

    //     std::panic::set_hook(Box::new(|info| {
    //         log::error!("Rust Runtime Panic: {}", info);
    //     }));
    // }

    dioxus::launch(App);
}

/// Helper function to safely locate the file in the OS sandbox
fn get_recording_path() -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push("clean_recording.wav");
    path
}

pub fn App() -> Element {
    // 2. UI State Signals
    let mut is_recording = use_signal(|| false);
    let mut is_playing = use_signal(|| false);
    let mut mic_permission_state = use_signal(|| "Unknown".to_string());
    let mut file_info_str = use_signal(|| "No recording found".to_string());

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

                // ==========================================
                // START PLAYBACK COMMAND
                // ==========================================
                AudioCommand::StartPlayback => {
                    // Stop ongoing recording stream to release hardware resources
                    record_stream = None;

                    let file_path = get_recording_path();
                    if let Ok(mut reader) = hound::WavReader::open(&file_path) {
                        // Load all floats directly into RAM (avoids blocking disk I/O in cpal callback)
                        let samples: Vec<f32> = reader.samples::<f32>().filter_map(Result::ok).collect();
                        let samples = Arc::new(samples);
                        let sample_index = Arc::new(AtomicUsize::new(0));

                        let sample_index_clone = Arc::clone(&sample_index);
                        let samples_clone = Arc::clone(&samples);

                        let host = cpal::default_host();
                        if let Some(device) = host.default_output_device() {
                            let config = cpal::StreamConfig {
                                channels: 1,
                                sample_rate: cpal::SampleRate(48000),
                                buffer_size: cpal::BufferSize::Default,
                            };

                            let new_playback_stream = device.build_output_stream(
                                &config,
                                move |data: &mut [f32], _: &_| {
                                    for sample in data.iter_mut() {
                                        let idx = sample_index_clone.fetch_add(1, Ordering::Relaxed);
                                        if idx < samples_clone.len() {
                                            *sample = samples_clone[idx];
                                        } else {
                                            *sample = 0.0; // Play silence if we reach EOF
                                        }
                                    }
                                },
                                |err| eprintln!("Playback stream hardware error: {}", err),
                                None,
                            ).expect("Failed to build output stream");

                            new_playback_stream.play().unwrap();
                            playback_stream = Some(new_playback_stream);
                        }
                    } else {
                        eprintln!("Error: Could not open clean_recording.wav for playback.");
                    }
                }

                // ==========================================
                // STOP PLAYBACK COMMAND
                // ==========================================
                AudioCommand::StopPlayback => {
                    playback_stream = None;
                }
            }
        }
    });

    rsx! {
        div {
            style: "
                display: flex;
                flex-direction: column;
                align-items: center;
                justify-content: center;
                min-height: 100vh;
                background: linear-gradient(135deg, #0f172a 0%, #1e293b 100%);
                font-family: system-ui, -apple-system, sans-serif;
                color: #f8fafc;
                padding: 24px;
            ",

            // Dashboard Container
            div {
                style: "
                    width: 100%;
                    max-width: 440px;
                    background: rgba(30, 41, 59, 0.7);
                    backdrop-filter: blur(12px);
                    border: 1px solid rgba(255, 255, 255, 0.08);
                    border-radius: 24px;
                    padding: 32px;
                    box-shadow: 0 20px 25px -5px rgba(0, 0, 0, 0.3), 0 10px 10px -5px rgba(0, 0, 0, 0.2);
                ",

                h2 {
                    style: "
                        font-size: 24px;
                        font-weight: 800;
                        text-align: center;
                        margin-top: 0;
                        margin-bottom: 24px;
                        background: linear-gradient(90deg, #38bdf8, #818cf8);
                        -webkit-background-clip: text;
                        -webkit-text-fill-color: transparent;
                        letter-spacing: -0.5px;
                    ",
                    "Acoustics Lab"
                }

                // Info Dashboard Panel
                div {
                    style: "
                        background: rgba(15, 23, 42, 0.6);
                        border-radius: 16px;
                        padding: 16px;
                        margin-bottom: 24px;
                        font-size: 14px;
                        border: 1px solid rgba(255, 255, 255, 0.03);
                    ",
                    div {
                        style: "display: flex; justify-content: space-between; margin-bottom: 8px;",
                        span { style: "color: #94a3b8;", "Microphone Input:" }
                        span {
                            style: "font-weight: 600; color: #38bdf8;",
                            "{mic_permission_state}"
                        }
                    }
                    div {
                        style: "display: flex; justify-content: space-between; margin-bottom: 8px;",
                        span { style: "color: #94a3b8;", "File Location:" }
                        span { style: "font-weight: 500; font-family: monospace; font-size: 12px;", "cache/clean_recording.wav" }
                    }
                    div {
                        style: "display: flex; justify-content: space-between;",
                        span { style: "color: #94a3b8;", "Recording Size:" }
                        span {
                            style: "font-weight: 600; color: #34d399;",
                            "{file_info_str}"
                        }
                    }
                }

                // Central Control Panel Buttons
                div {
                    style: "display: flex; flex-direction: column; gap: 14px;",

                    // 1. MIC PERMISSION INITIATOR
                    button {
                        style: "
                            background: rgba(56, 189, 248, 0.1);
                            color: #38bdf8;
                            border: 1px solid rgba(56, 189, 248, 0.3);
                            border-radius: 12px;
                            padding: 12px 16px;
                            font-size: 14px;
                            font-weight: 600;
                            cursor: pointer;
                            transition: all 0.2s ease;
                        ",
                        onclick: move |_| async move {
                            mic_permission_state.set("Requesting...".into());
                            let granted = dioxus_recorder::ensure().await;
                            mic_permission_state.set(if granted { "Granted".into() } else { "Denied".into() });
                        },
                        "Initialize Microphone Hardware"
                    }

                    // 2. RECORD / STOP RECORD BUTTON
                    button {
                        style: if is_recording() {
                            "
                                background: #ef4444;
                                color: #ffffff;
                                border: none;
                                border-radius: 12px;
                                padding: 14px 20px;
                                font-size: 15px;
                                font-weight: 700;
                                cursor: pointer;
                                transition: all 0.2s ease;
                                box-shadow: 0 4px 12px rgba(239, 68, 68, 0.3);
                            "
                        } else {
                            "
                                background: #3b82f6;
                                color: #ffffff;
                                border: none;
                                border-radius: 12px;
                                padding: 14px 20px;
                                font-size: 15px;
                                font-weight: 700;
                                cursor: pointer;
                                transition: all 0.2s ease;
                                box-shadow: 0 4px 12px rgba(59, 130, 246, 0.3);
                            "
                        },
                        onclick: move |_| {
                            if is_recording() {
                                audio_task.send(AudioCommand::StopRecord);
                                is_recording.set(false);
                                // Refresh recording file size
                                refresh_file_metadata();
                            } else {
                                // If playing, shut down playback streams first
                                if is_playing() {
                                    audio_task.send(AudioCommand::StopPlayback);
                                    is_playing.set(false);
                                }
                                audio_task.send(AudioCommand::StartRecord);
                                is_recording.set(true);
                            }
                        },
                        if is_recording() { "■  Stop Denoised Recording" } else { "●  Start Denoised Recording" }
                    }

                    // 3. PLAY / STOP PLAYBACK BUTTON
                    button {
                        style: if is_playing() {
                            "
                                background: #e2e8f0;
                                color: #0f172a;
                                border: none;
                                border-radius: 12px;
                                padding: 14px 20px;
                                font-size: 15px;
                                font-weight: 700;
                                cursor: pointer;
                                transition: all 0.2s ease;
                                box-shadow: 0 4px 12px rgba(226, 232, 240, 0.2);
                            "
                        } else {
                            "
                                background: #10b981;
                                color: #ffffff;
                                border: none;
                                border-radius: 12px;
                                padding: 14px 20px;
                                font-size: 15px;
                                font-weight: 700;
                                cursor: pointer;
                                transition: all 0.2s ease;
                                box-shadow: 0 4px 12px rgba(16, 185, 129, 0.3);
                            "
                        },
                        onclick: move |_| {
                            if is_playing() {
                                audio_task.send(AudioCommand::StopPlayback);
                                is_playing.set(false);
                            } else {
                                // If recording, shut down recording stream first
                                if is_recording() {
                                    audio_task.send(AudioCommand::StopRecord);
                                    is_recording.set(false);
                                }
                                audio_task.send(AudioCommand::StartPlayback);
                                is_playing.set(true);
                            }
                        },
                        if is_playing() { "■  Stop Local Playback" } else { "▶  Listen to Denoised Audio" }
                    }
                }
            }
        }
    }
}

fn tragedies_happen(err: cpal::StreamError) {
    eprintln!("Audio input device dropped unexpectedly: {}", err);
}
