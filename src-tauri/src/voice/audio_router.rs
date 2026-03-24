use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;

// Audio routing modes (lock-free via atomic)
const MODE_INACTIVE: u8 = 0;
const MODE_WAKEWORD: u8 = 1;
const MODE_PTT: u8 = 2;

/// Single-producer single-consumer ring buffer for the real-time audio callback.
/// No mutex, no allocation -- writes via atomic write_pos.
pub struct SpscRingBuffer {
    buffer: Box<[f32]>,
    write_pos: AtomicUsize,
    capacity: usize,
}

impl SpscRingBuffer {
    pub fn new(capacity: usize) -> Self {
        SpscRingBuffer {
            buffer: vec![0.0f32; capacity].into_boxed_slice(),
            write_pos: AtomicUsize::new(0),
            capacity,
        }
    }

    /// Write samples into the ring buffer (called from cpal audio callback).
    /// Lock-free: only uses atomic store on write_pos.
    pub fn write(&self, samples: &[f32]) {
        let mut pos = self.write_pos.load(Ordering::Relaxed);
        // Safety: we're the only writer (single-producer).
        // We cast away shared ref to write -- this is safe because:
        // 1. Only one thread (the cpal callback) ever calls write()
        // 2. Readers snapshot data at a consistent write_pos
        let buf = self.buffer.as_ptr() as *mut f32;
        for &s in samples {
            unsafe { *buf.add(pos % self.capacity) = s; }
            pos += 1;
        }
        self.write_pos.store(pos, Ordering::Release);
    }

    /// Read the most recent `count` samples. Called from consumer threads.
    /// Returns fewer samples if the buffer hasn't been filled that much yet.
    pub fn read_last(&self, count: usize) -> Vec<f32> {
        let wp = self.write_pos.load(Ordering::Acquire);
        let available = wp.min(self.capacity);
        let to_read = count.min(available);
        if to_read == 0 {
            return Vec::new();
        }

        let start = if wp >= to_read { wp - to_read } else { 0 };
        let mut result = Vec::with_capacity(to_read);
        for i in start..start + to_read {
            result.push(self.buffer[i % self.capacity]);
        }
        result
    }

    /// Reset write position (e.g., when starting a new PTT session).
    pub fn reset(&self) {
        self.write_pos.store(0, Ordering::Release);
    }

    /// Read all written samples since last reset, up to capacity.
    pub fn read_all(&self) -> Vec<f32> {
        let wp = self.write_pos.load(Ordering::Acquire);
        let count = wp.min(self.capacity);
        let mut result = Vec::with_capacity(count);
        let start = if wp > self.capacity { wp - self.capacity } else { 0 };
        for i in start..start + count {
            result.push(self.buffer[i % self.capacity]);
        }
        result
    }
}

// Safety: SpscRingBuffer uses atomic operations for synchronization.
// The buffer data is only written by one thread (cpal callback) and
// read by another (consumer), with proper Acquire/Release ordering.
unsafe impl Send for SpscRingBuffer {}
unsafe impl Sync for SpscRingBuffer {}

/// Shared audio router: one cpal stream, lock-free callback, mode-based routing.
pub struct AudioRouter {
    stream: Option<cpal::Stream>,
    mode: Arc<AtomicU8>,
    /// Ring buffer for wake word detection (fixed 2-second window)
    ring: Arc<SpscRingBuffer>,
    /// Buffer for push-to-talk recording (max 30 seconds)
    ptt_buf: Arc<SpscRingBuffer>,
    /// Actual device sample rate (may differ from 16kHz)
    sample_rate: u32,
}

impl AudioRouter {
    pub fn new() -> Self {
        // Ring buffer: 2 seconds at 48kHz (common native rate) = 96000 samples
        // If device is 16kHz, this holds ~6 seconds -- that's fine, more headroom
        let ring = Arc::new(SpscRingBuffer::new(96_000));
        // PTT buffer: 30 seconds at 48kHz = 1_440_000 samples
        let ptt_buf = Arc::new(SpscRingBuffer::new(1_440_000));

        AudioRouter {
            stream: None,
            mode: Arc::new(AtomicU8::new(MODE_INACTIVE)),
            ring,
            ptt_buf,
            sample_rate: 16000, // will be updated on start()
        }
    }

    /// Open the cpal input stream using the device's native sample rate.
    /// The stream stays open for the app's lifetime.
    pub fn start(&mut self) -> Result<(), String> {
        if self.stream.is_some() {
            return Ok(()); // already running
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("No input device available")?;

        let device_name = device.description().map(|d| d.name().to_string()).unwrap_or_default();
        log::info!("AudioRouter: using input device: {}", device_name);

        // Use device's supported config for maximum compatibility
        let supported = device
            .default_input_config()
            .map_err(|e| format!("Failed to get input config: {}", e))?;

        self.sample_rate = supported.sample_rate();
        log::info!("AudioRouter: device sample rate: {} Hz", self.sample_rate);

        let config = cpal::StreamConfig {
            channels: 1,
            sample_rate: self.sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        let mode = Arc::clone(&self.mode);
        let ring = Arc::clone(&self.ring);
        let ptt_buf = Arc::clone(&self.ptt_buf);

        let stream = device
            .build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // Lock-free: only atomic load + ring buffer write
                    let m = mode.load(Ordering::Relaxed);
                    match m {
                        MODE_WAKEWORD => ring.write(data),
                        MODE_PTT => ptt_buf.write(data),
                        _ => {} // MODE_INACTIVE: discard
                    }
                },
                |err| log::error!("AudioRouter stream error: {}", err),
                None,
            )
            .map_err(|e| format!("Failed to build input stream: {}", e))?;

        stream
            .play()
            .map_err(|e| format!("Failed to start stream: {}", e))?;

        self.stream = Some(stream);
        log::info!("AudioRouter: stream started");
        Ok(())
    }

    /// Set the routing mode (atomic, instant).
    pub fn set_mode(&self, mode: u8) {
        self.mode.store(mode, Ordering::Relaxed);
    }

    /// Start push-to-talk recording. Resets PTT buffer and switches mode.
    pub fn start_ptt(&self) {
        self.ptt_buf.reset();
        self.mode.store(MODE_PTT, Ordering::Relaxed);
    }

    /// Stop push-to-talk. Returns recorded samples (resampled to 16kHz if needed).
    /// Switches mode back to wake word listening.
    pub fn stop_ptt(&self) -> Vec<f32> {
        self.mode.store(MODE_WAKEWORD, Ordering::Relaxed);
        let raw = self.ptt_buf.read_all();
        self.resample_to_16k(raw)
    }

    /// Read the current PTT buffer without stopping the recording session.
    pub fn peek_ptt(&self) -> Vec<f32> {
        let raw = self.ptt_buf.read_all();
        self.resample_to_16k(raw)
    }

    /// Read the last N seconds from the ring buffer (for wake word detection).
    /// Returns samples resampled to 16kHz.
    pub fn read_ring(&self, duration_secs: f32) -> Vec<f32> {
        let sample_count = (duration_secs * self.sample_rate as f32) as usize;
        let raw = self.ring.read_last(sample_count);
        self.resample_to_16k(raw)
    }

    /// Mute capture (set mode to inactive). Used during TTS to prevent feedback.
    pub fn mute(&self) {
        self.mode.store(MODE_INACTIVE, Ordering::Relaxed);
    }

    /// Unmute capture (set mode back to wake word listening).
    pub fn unmute(&self) {
        self.mode.store(MODE_WAKEWORD, Ordering::Relaxed);
    }

    /// Stop routing audio into either wake-word or push-to-talk buffers.
    pub fn deactivate(&self) {
        self.mode.store(MODE_INACTIVE, Ordering::Relaxed);
    }

    /// Check if the stream is alive.
    pub fn is_alive(&self) -> bool {
        self.stream.is_some()
    }

    /// Reconnect: drop and re-open the cpal stream.
    pub fn reconnect(&mut self) -> Result<(), String> {
        log::info!("AudioRouter: reconnecting...");
        self.stream = None;
        self.start()
    }

    /// Get the actual device sample rate.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Resample from device rate to 16kHz (what Whisper expects).
    /// If already 16kHz, returns as-is. Uses simple linear interpolation.
    fn resample_to_16k(&self, samples: Vec<f32>) -> Vec<f32> {
        if self.sample_rate == 16000 || samples.is_empty() {
            return samples;
        }

        let ratio = 16000.0 / self.sample_rate as f64;
        let out_len = (samples.len() as f64 * ratio) as usize;
        let mut out = Vec::with_capacity(out_len);

        for i in 0..out_len {
            let src_pos = i as f64 / ratio;
            let idx = src_pos as usize;
            let frac = (src_pos - idx as f64) as f32;

            if idx + 1 < samples.len() {
                out.push(samples[idx] * (1.0 - frac) + samples[idx + 1] * frac);
            } else if idx < samples.len() {
                out.push(samples[idx]);
            }
        }

        out
    }
}
