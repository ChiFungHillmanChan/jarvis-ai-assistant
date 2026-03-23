use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

pub struct AudioCapture {
    stream: Option<cpal::Stream>,
    buffer: Arc<Mutex<Vec<f32>>>,
    is_recording: Arc<Mutex<bool>>,
}

impl AudioCapture {
    pub fn new() -> Result<Self, String> {
        Ok(AudioCapture {
            stream: None,
            buffer: Arc::new(Mutex::new(Vec::new())),
            is_recording: Arc::new(Mutex::new(false)),
        })
    }

    pub fn start_recording(&mut self) -> Result<(), String> {
        let host = cpal::default_host();
        let device = host.default_input_device()
            .ok_or("No input device available")?;

        let device_name = device.description().map(|d| d.name().to_string()).unwrap_or_default();
        log::info!("Using input device: {}", device_name);

        let config = cpal::StreamConfig {
            channels: 1,
            sample_rate: 16000,
            buffer_size: cpal::BufferSize::Default,
        };

        let buffer = Arc::clone(&self.buffer);
        let is_recording = Arc::clone(&self.is_recording);

        buffer.lock().unwrap().clear();
        *is_recording.lock().unwrap() = true;

        let stream = device.build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                if *is_recording.lock().unwrap() {
                    buffer.lock().unwrap().extend_from_slice(data);
                }
            },
            |err| log::error!("Audio capture error: {}", err),
            None,
        ).map_err(|e| format!("Failed to build input stream: {}", e))?;

        stream.play().map_err(|e| format!("Failed to start stream: {}", e))?;
        self.stream = Some(stream);

        log::info!("Audio capture started");
        Ok(())
    }

    pub fn stop_recording(&mut self) -> Vec<f32> {
        *self.is_recording.lock().unwrap() = false;
        self.stream = None;

        let samples = self.buffer.lock().unwrap().clone();
        self.buffer.lock().unwrap().clear();

        log::info!("Audio capture stopped: {} samples ({:.1}s)", samples.len(), samples.len() as f32 / 16000.0);
        samples
    }

    pub fn is_recording(&self) -> bool {
        *self.is_recording.lock().unwrap()
    }
}
