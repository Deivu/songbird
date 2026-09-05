use crate::constants::SAMPLE_RATE;
use crate::driver::{Filter, FilterChain, Pcm};
use std::collections::VecDeque;
use tracing::error;

#[derive(Default)]
pub struct FilterState {
    filters: FilterChain,
    output: Vec<f32>,
    output_frames: usize,
    pending_output: VecDeque<f32>,
    channels: usize,
}

impl FilterState {
    pub fn update(&mut self, updates: Vec<Filter>, input_frames: usize) {
        if self.filters.is_empty() {
            return;
        }

        for filter in updates {
            if let Err(e) = filter.apply(&mut self.filters) {
                error!("Failed to update filter {:?}", e);
            }
        }

        let mut output_frames = input_frames;
        let mut max_output_frames = input_frames;

        for filter in self.filters.filters() {
            output_frames = filter.max_output_frames(output_frames);
            max_output_frames = max_output_frames.max(output_frames);
        }

        self.output.resize(max_output_frames * 2, 0.0);
        self.output_frames = max_output_frames;

        let required_samples = (input_frames + max_output_frames) * 2;

        if self.pending_output.capacity() < required_samples {
            self.pending_output
                .reserve(required_samples.saturating_sub(self.pending_output.len()));
        }
    }

    pub fn process(
        &mut self,
        input: &[f32],
        input_frames: usize,
        channels: usize,
    ) -> Option<usize> {
        self.update_channels(channels);

        let input_samples = input_frames.checked_mul(channels)?;

        if input.len() < input_samples || self.output.len() < input_samples {
            return None;
        }

        self.output[..input_samples].copy_from_slice(&input[..input_samples]);

        let mut output_frames = input_frames;

        for filter in self.filters.filters_mut() {
            output_frames = filter.process(Pcm {
                samples: &mut self.output,
                frames: output_frames,
                sample_rate: SAMPLE_RATE,
                channels: channels as u32,
            });

            if output_frames > self.output_frames {
                return None;
            }
        }

        let output_samples = output_frames.checked_mul(channels)?;

        if output_samples > self.pending_output.capacity() - self.pending_output.len() {
            return None;
        }

        self.pending_output
            .extend(self.output[..output_samples].iter().copied());

        Some(output_frames)
    }

    pub fn copy_output(&mut self, output: &mut [f32], channels: usize) -> usize {
        self.update_channels(channels);

        let samples_to_copy = self.pending_output.len().min(output.len());

        for sample in &mut output[..samples_to_copy] {
            *sample = self.pending_output.pop_front().unwrap();
        }

        output[samples_to_copy..].fill(0.0);

        samples_to_copy / channels
    }

    pub fn has_filters(&self) -> bool {
        !self.filters.is_empty()
    }

    pub fn queued_samples(&self) -> usize {
        self.pending_output.len()
    }

    pub fn is_empty(&self) -> bool {
        self.filters.is_empty() && self.pending_output.is_empty()
    }

    fn update_channels(&mut self, channels: usize) {
        if self.channels != channels {
            self.pending_output.clear();
            self.channels = channels;
        }
    }
}
