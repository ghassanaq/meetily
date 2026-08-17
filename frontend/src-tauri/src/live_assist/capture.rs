use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Stream, SupportedStreamConfig};

use crate::audio::{default_output_device, get_device_and_config};

pub const BUFFER_SECONDS: usize = 60;
pub const PRE_ROLL_SECONDS: usize = 4;
const RECEIVING_GRACE: Duration = Duration::from_millis(1_500);

#[derive(Debug, Clone, Copy)]
pub struct CaptureMarker {
    signal_sample_index: u64,
    signal_at: Instant,
}

#[derive(Debug)]
pub struct CapturedClip {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub capture_ms: u64,
}

#[derive(Debug)]
pub struct CaptureBuffer {
    samples: VecDeque<f32>,
    sample_rate: u32,
    max_samples: usize,
    total_samples: u64,
    last_callback: Option<Instant>,
    level_rms: f32,
}

impl Default for CaptureBuffer {
    fn default() -> Self {
        Self {
            samples: VecDeque::new(),
            sample_rate: 0,
            max_samples: 0,
            total_samples: 0,
            last_callback: None,
            level_rms: 0.0,
        }
    }
}

impl CaptureBuffer {
    fn configure(&mut self, sample_rate: u32) {
        if self.sample_rate != sample_rate {
            self.samples.clear();
            self.total_samples = 0;
        }
        self.sample_rate = sample_rate;
        self.max_samples = sample_rate as usize * BUFFER_SECONDS;
    }

    fn push_interleaved(&mut self, samples: &[f32], channels: u16, sample_rate: u32) {
        self.configure(sample_rate);
        let channels = usize::from(channels.max(1));
        let mut sum_squares = 0.0_f32;
        let mut count = 0_usize;

        for frame in samples.chunks(channels) {
            let mono = frame.iter().copied().sum::<f32>() / frame.len() as f32;
            self.samples.push_back(mono);
            self.total_samples = self.total_samples.saturating_add(1);
            sum_squares += mono * mono;
            count += 1;
        }
        while self.samples.len() > self.max_samples {
            self.samples.pop_front();
        }

        self.level_rms = if count == 0 {
            0.0
        } else {
            (sum_squares / count as f32).sqrt()
        };
        self.last_callback = Some(Instant::now());
    }

    pub fn marker(&self) -> Result<CaptureMarker> {
        if self.sample_rate == 0 || self.last_callback.is_none() {
            return Err(anyhow!("assist audio stream is not receiving samples"));
        }
        Ok(CaptureMarker {
            signal_sample_index: self.total_samples,
            signal_at: Instant::now(),
        })
    }

    pub fn extract(&self, marker: CaptureMarker) -> Result<CapturedClip> {
        if self.sample_rate == 0 {
            return Err(anyhow!("assist audio stream is not configured"));
        }
        let ring_start = self.total_samples.saturating_sub(self.samples.len() as u64);
        let pre_roll = self.sample_rate as u64 * PRE_ROLL_SECONDS as u64;
        let requested_start = marker.signal_sample_index.saturating_sub(pre_roll);
        if requested_start < ring_start {
            return Err(anyhow!(
                "capture exceeded the {} second in-memory retention window",
                BUFFER_SECONDS
            ));
        }
        let start_offset = requested_start.saturating_sub(ring_start) as usize;
        let output: Vec<f32> = self.samples.iter().skip(start_offset).copied().collect();
        if output.is_empty() {
            return Err(anyhow!("captured clip is empty"));
        }
        Ok(CapturedClip {
            samples: output,
            sample_rate: self.sample_rate,
            capture_ms: marker
                .signal_at
                .elapsed()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
        })
    }

    pub fn health(&self) -> (bool, f32) {
        let receiving = self
            .last_callback
            .is_some_and(|last| last.elapsed() <= RECEIVING_GRACE);
        (receiving, self.level_rms)
    }
}

pub struct AssistAudioStream {
    stream: Stream,
    pub sample_rate: u32,
}

// Matches the existing AudioStream safety boundary: the stream is stored behind
// a mutex and is only paused/dropped by the application lifecycle.
unsafe impl Send for AssistAudioStream {}

impl AssistAudioStream {
    pub async fn open(buffer: Arc<Mutex<CaptureBuffer>>) -> Result<Self> {
        let device = default_output_device()?;
        let (cpal_device, config) = get_device_and_config(&device).await?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        buffer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .configure(sample_rate);
        let stream = build_stream(&cpal_device, &config, buffer, channels, sample_rate)?;
        stream.play()?;
        Ok(Self {
            stream,
            sample_rate,
        })
    }

    pub fn stop(self) -> Result<()> {
        self.stream.pause()?;
        drop(self.stream);
        Ok(())
    }
}

fn build_stream(
    device: &cpal::Device,
    config: &SupportedStreamConfig,
    buffer: Arc<Mutex<CaptureBuffer>>,
    channels: u16,
    sample_rate: u32,
) -> Result<Stream> {
    let stream_config = config.clone().into();
    let error_callback = |error| log::error!("Live Assist audio stream error: {error}");
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let sink = buffer.clone();
            device.build_input_stream(
                &stream_config,
                move |data: &[f32], _| push(&sink, data, channels, sample_rate),
                error_callback,
                None,
            )?
        }
        cpal::SampleFormat::I16 => {
            let sink = buffer.clone();
            device.build_input_stream(
                &stream_config,
                move |data: &[i16], _| {
                    let converted: Vec<f32> = data
                        .iter()
                        .map(|sample| *sample as f32 / i16::MAX as f32)
                        .collect();
                    push(&sink, &converted, channels, sample_rate);
                },
                error_callback,
                None,
            )?
        }
        cpal::SampleFormat::I32 => {
            let sink = buffer.clone();
            device.build_input_stream(
                &stream_config,
                move |data: &[i32], _| {
                    let converted: Vec<f32> = data
                        .iter()
                        .map(|sample| *sample as f32 / i32::MAX as f32)
                        .collect();
                    push(&sink, &converted, channels, sample_rate);
                },
                error_callback,
                None,
            )?
        }
        cpal::SampleFormat::I8 => {
            let sink = buffer;
            device.build_input_stream(
                &stream_config,
                move |data: &[i8], _| {
                    let converted: Vec<f32> = data
                        .iter()
                        .map(|sample| *sample as f32 / i8::MAX as f32)
                        .collect();
                    push(&sink, &converted, channels, sample_rate);
                },
                error_callback,
                None,
            )?
        }
        format => return Err(anyhow!("unsupported Live Assist sample format: {format:?}")),
    };
    Ok(stream)
}

fn push(buffer: &Arc<Mutex<CaptureBuffer>>, data: &[f32], channels: u16, sample_rate: u32) {
    buffer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push_interleaved(data, channels, sample_rate);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_extracts_pre_roll_and_post_signal_samples() {
        let mut buffer = CaptureBuffer::default();
        buffer.push_interleaved(&[1.0; 100], 1, 10);
        let marker = buffer.marker().unwrap();
        buffer.push_interleaved(&[2.0; 20], 1, 10);
        let clip = buffer.extract(marker).unwrap();
        assert_eq!(clip.sample_rate, 10);
        assert_eq!(clip.samples.len(), 60);
        assert_eq!(&clip.samples[..40], &[1.0; 40]);
        assert_eq!(&clip.samples[40..], &[2.0; 20]);
    }

    #[test]
    fn interleaved_channels_are_downmixed_before_retention() {
        let mut buffer = CaptureBuffer::default();
        buffer.push_interleaved(&[1.0, -1.0, 0.5, 0.5], 2, 10);
        assert_eq!(
            buffer.samples.iter().copied().collect::<Vec<_>>(),
            [0.0, 0.5]
        );
    }
}
