use crate::{
    constants::STEREO_FRAME_SIZE,
    driver::{AudioFilter, FilterRegistry},
};
use std::{any::Any, mem, ops::Range};
use tracing::{error, warn};

struct FilterEntry {
    name: String,
    filter: Box<dyn AudioFilter>,
    failing: bool,
}

#[derive(Default)]
pub struct FilterChain {
    entries: Vec<FilterEntry>,
    current: Vec<f32>,
    next: Vec<f32>,
    tail: Range<usize>,
}

impl FilterChain {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn set(&mut self, filters: Vec<(String, Box<dyn Any + Send>)>, registry: &FilterRegistry) {
        let mut entries = Vec::with_capacity(filters.len());

        for (name, config) in filters {
            if entries.iter().any(|entry: &FilterEntry| entry.name == name) {
                warn!(
                    "Filter {} was given more than once, keeping the first",
                    name
                );
                continue;
            }

            let existing = self.entries.iter().position(|entry| entry.name == name);
            let mut entry = if let Some(index) = existing {
                self.entries.swap_remove(index)
            } else {
                match registry.create(&name) {
                    Some(Ok(filter)) => FilterEntry {
                        name,
                        filter,
                        failing: false,
                    },
                    Some(Err(e)) => {
                        error!("Filter {} could not be created: {}", name, e);
                        continue;
                    },
                    None => {
                        warn!("No filter named {}", name);
                        continue;
                    },
                }
            };

            if let Err(e) = entry.filter.configure(config) {
                error!("Filter {} rejected its config: {}", entry.name, e);
            }

            entries.push(entry);
        }

        self.entries = entries;

        if self.entries.is_empty() {
            self.tail = 0..0;
        } else if self.current.is_empty() {
            self.current = vec![0.0; STEREO_FRAME_SIZE];
            self.next = vec![0.0; STEREO_FRAME_SIZE];
        }
    }

    // Audio which the output has no room for is kept in `next`, and returned by the
    // following calls before any filter runs. Callers drain the chain with an empty
    // input until it returns 0 before passing new audio.
    pub fn process(&mut self, input: &[f32], output: &mut [f32], channels: usize) -> usize {
        if !self.tail.is_empty() {
            debug_assert!(input.is_empty());

            let end = self.tail.end.min(self.tail.start + output.len());
            let held = &self.next[self.tail.start..end];

            output[..held.len()].copy_from_slice(held);
            self.tail.start = end;

            return held.len() / channels;
        }

        let remaining = output.len() / channels;
        let buffer_frames = self.next.len() / channels;
        let last = self.entries.len().saturating_sub(1);
        let mut frames = input.len() / channels;

        for (index, entry) in self.entries.iter_mut().enumerate() {
            let source = if index == 0 {
                &input[..frames * channels]
            } else {
                &self.current[..frames * channels]
            };
            let overflows = index == last && frames > remaining;
            let target = if index == last && !overflows {
                &mut output[..]
            } else {
                let capacity = frames.max(remaining).min(buffer_frames);

                &mut self.next[..capacity * channels]
            };
            let capacity = target.len() / channels;

            frames = match entry.filter.process(source, target, channels) {
                Ok(written) => {
                    entry.failing = false;

                    written.min(capacity)
                },
                Err(e) => {
                    if !entry.failing {
                        error!("Filter {} failed, passing audio through: {}", entry.name, e);
                        entry.failing = true;
                    }

                    let passed = frames.min(capacity);
                    target[..passed * channels].copy_from_slice(&source[..passed * channels]);

                    passed
                },
            };

            if index != last {
                mem::swap(&mut self.current, &mut self.next);
            } else if overflows {
                let written = frames * channels;
                let fits = written.min(remaining * channels);

                output[..fits].copy_from_slice(&self.next[..fits]);
                self.tail = fits..written;
                frames = fits / channels;
            }
        }

        frames
    }

    pub fn reset(&mut self) {
        self.tail = 0..0;

        for entry in &mut self.entries {
            entry.filter.reset();
        }
    }
}
