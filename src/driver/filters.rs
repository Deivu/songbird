use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::marker::PhantomData;

trait FilterUpdate: Send {
    fn apply(self: Box<Self>, chain: &mut FilterChain) -> Result<(), serde_json::Error>;
}

struct Set<T: AudioFilter> {
    config: T::Config,
}

struct Remove<T: AudioFilter> {
    marker: PhantomData<T>,
}

impl<T: AudioFilter> FilterUpdate for Set<T> {
    fn apply(self: Box<Self>, chain: &mut FilterChain) -> Result<(), serde_json::Error> {
        let Set { config } = *self;

        chain.update::<T>(config)
    }
}

impl<T: AudioFilter> FilterUpdate for Remove<T> {
    fn apply(self: Box<Self>, chain: &mut FilterChain) -> Result<(), serde_json::Error> {
        chain.remove::<T>();

        Ok(())
    }
}

pub struct Filter {
    update: Box<dyn FilterUpdate>,
}

impl Filter {
    /// Creates or Updates an existing filter
    pub fn update<T: AudioFilter>(config: T::Config) -> Self {
        Self {
            update: Box::new(Set::<T> { config }),
        }
    }

    /// Removes an active filter
    pub fn remove<T: AudioFilter>() -> Self {
        Self {
            update: Box::new(Remove::<T> {
                marker: PhantomData,
            }),
        }
    }

    /// Applies the new configuration for this filter
    pub(crate) fn apply(
        self,
        filters: &mut FilterChain,
    ) -> Result<(), serde_json::Error> {
        self.update.apply(filters)
    }
}

/// A PCM slice, that contains additional information for filters to process
pub struct Pcm<'a> {
    pub samples: &'a mut [f32],
    pub frames: usize,
    pub sample_rate: u32,
    pub channels: u32,
}

/// A vec of filters currently active in this struct
#[derive(Default)]
pub struct FilterChain {
    filters: Vec<Box<dyn ActiveFilter>>,
}

impl FilterChain {
    /// Updates a filter configuration via it's AudioFilter impl
    pub fn update<T: AudioFilter>(
        &mut self,
        config: T::Config,
    ) -> Result<(), serde_json::Error> {
        if let Some(filter) = self
            .filters
            .iter_mut()
            .find(|filter| filter.name() == T::NAME)
        {
            return filter.update_value(serde_json::to_value(config)?);
        }

        let mut filter = T::default();
        AudioFilter::update(&mut filter, config);

        self.filters.push(Box::new(filter));

        Ok(())
    }

    /// Removes a filter from the filter chain via it's AudioFilter impl
    pub fn remove<T: AudioFilter>(&mut self) {
        self.filters.retain(|filter| filter.name() != T::NAME);
    }

    /// If current filter chain is empty
    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }

    /// List of immutable active filters in this chain
    pub(crate) fn filters(&self) -> &[Box<dyn ActiveFilter>] {
        &self.filters
    }

    /// List of mutable active filters in this chain
    pub(crate) fn filters_mut(&mut self) -> &mut [Box<dyn ActiveFilter>] {
        &mut self.filters
    }
}

/// Trait for creating a new filter instance. A filter instance must support implement Default,
/// As default is used to initialize the instance "Self" where it can store its configuration.
pub trait AudioFilter: Default + Send + 'static {
    type Config: Serialize + DeserializeOwned + Send + 'static;

    /// Name for this filter
    const NAME: &'static str;

    /// Updates the stored filter settings
    fn update(&mut self, config: Self::Config);

    /// The max output frames expected to be generated from this Filter
    fn max_output_frames(&self, input_frames: usize) -> usize {
        input_frames
    }

    /// Process a PCM f32 buffer, and store the result in output buffer
    fn process(&mut self, output: Pcm<'_>) -> usize;
}

/// Wraps around AudioFilter, so Songbird can store filters with one defined type in a Vec
pub(crate) trait ActiveFilter: Send {
    fn name(&self) -> &'static str;

    fn update_value(&mut self, config: Value) -> Result<(), serde_json::Error>;

    fn max_output_frames(&self, input_frames: usize) -> usize;

    fn process(&mut self, output: Pcm<'_>) -> usize;
}

impl<T: AudioFilter> ActiveFilter for T {
    fn name(&self) -> &'static str {
        T::NAME
    }

    fn update_value(&mut self, config: Value) -> Result<(), serde_json::Error> {
        let config = serde_json::from_value::<T::Config>(config)?;

        AudioFilter::update(self, config);

        Ok(())
    }

    fn max_output_frames(&self, input_frames: usize) -> usize {
        AudioFilter::max_output_frames(self, input_frames)
    }

    fn process(&mut self, output: Pcm<'_>) -> usize {
        AudioFilter::process(self, output)
    }
}
