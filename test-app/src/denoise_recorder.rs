use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use dioxus::prelude::*;
use futures_util::StreamExt;
use hound::{WavSpec, WavWriter};
use nnnoiseless::DenoiseState;
use ringbuf::{storage::Heap, traits::*, SharedRb};
use std::fs::File;
use std::io::BufWriter;
use std::sync::{Arc, Mutex};

enum AudioCommand {
    Start,
    Stop,
}

pub fn CleanAudioRecorder() -> Element {
    let mut is_recording = use_signal(|| false);

    let audio_task = use_coroutine(move |mut rx: UnboundedReceiver<AudioCommand>| async move {
        let mut stream: Option<cpal::Stream> = None;
        let writer_mutex: Arc<Mutex<Option<WavWriter<BufWriter<File>>>>> = Arc::new(Mutex::new(None));

        while let Some(cmd) = rx.next().await {
            match cmd {
                AudioCommand::Start => {
                    let host = cpal::default_host();
                    let device = host.default_input_device().expect("No mic found");

                    // FORCE 48kHz configuration
                    let config = cpal::StreamConfig {
                        channels: 1, // nnnoiseless operates on mono audio
                        sample_rate: cpal::SampleRate(48000),
                        buffer_size: cpal::BufferSize::Default,
                    };

                    let spec = WavSpec {
                        channels: 1,
                        sample_rate: 48000,
                        bits_per_sample: 32,
                        sample_format: hound::SampleFormat::Float,
                    };

                    let writer = WavWriter::create("clean_recording.wav", spec).unwrap();
                    *writer_mutex.lock().unwrap() = Some(writer);

                    // Initialize nnnoiseless state
                    let mut denoiser = DenoiseState::new();

                    // Create a thread-safe ring buffer.
                    // 4096 samples provides a comfortable cushion against I/O jitter.
                    let rb = SharedRb::<Heap<f32>>::new(4096);
                    let (mut prod, mut cons) = rb.split();

                    let writer_clone = Arc::clone(&writer_mutex);

                    // 1. Microphone Input Callback (High-Priority OS Thread)
                    let new_stream = device.build_input_stream(
                        &config,
                        move |data: &[f32], _: &_| {
                            // Shove raw mic data into the ring buffer as fast as possible
                            let _ = prod.push_slice(data);

                            // Initialize our processing frames
                            let mut input_frame = [0.0f32; 480];
                            let mut output_frame = [0.0f32; 480];

                            // Pull out data in exact 480-sample blocks to denoise
                            while cons.len() >= 480 {
                                cons.pop_slice(&mut input_frame);

                                // Magic happens here: Background noise stripped out instantly
                                denoiser.process_frame(&mut output_frame, &input_frame);

                                // Write the pristine, denoised frames to disk
                                if let Ok(mut guard) = writer_clone.lock() {
                                    if let Some(w) = guard.as_mut() {
                                        for &sample in &output_frame {
                                            w.write_sample(sample).unwrap();
                                        }
                                    }
                                }
                            }
                        },
                        |err| tragedies_happen(err),
                        None,
                    ).expect("Failed to build 48kHz stream");

                    new_stream.play().unwrap();
                    stream = Some(new_stream);
                }
                AudioCommand::Stop => {
                    stream = None;
                    if let Ok(mut guard) = writer_mutex.lock() {
                        if let Some(w) = guard.take() {
                            w.finalize().unwrap();
                        }
                    }
                }
            }
        }
    });

    rsx! {
        div { padding: "20px", font_family: "sans-serif",
            h2 { "Denoised 48kHz Audio Recorder" }
            button {
                padding: "10px 20px",
                onclick: move |_| {
                    if is_recording() {
                        audio_task.send(AudioCommand::Stop);
                        is_recording.set(false);
                    } else {
                        audio_task.send(AudioCommand::Start);
                        is_recording.set(true);
                    }
                },
                "{if is_recording() { \"Stop\" } else { \"Start Denoised Recording\" }}"
            }
        }
    }
}

fn tragedies_happen(err: cpal::StreamError) {
    eprintln!("Audio stream error: {}", err);
}
