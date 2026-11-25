use std::{
    any::Any,
    collections::HashMap,
    error::Error,
    fmt::{Debug, Formatter, Result as FmtResult},
    sync::Arc,
};

/// Error returned by an [`AudioFilter`].
pub type FilterError = Box<dyn Error + Send + Sync>;

/// Creates a new instance of one kind of filter.
///
/// Registered under a name with [`Config::add_filter`], and called when
/// [`Driver::set_filters`] names a filter which is not yet active. Returning an error
/// skips the filter.
///
/// [`Config::add_filter`]: crate::Config::add_filter
/// [`Driver::set_filters`]: crate::driver::Driver::set_filters
pub type FilterConstructor =
    Arc<dyn Fn() -> Result<Box<dyn AudioFilter>, FilterError> + Send + Sync>;

/// The filters a driver can create, by name.
///
/// Filters from different sources, such as the application itself and a plugin
/// library, are registered side by side. Registering a name again replaces it.
#[derive(Clone, Default)]
pub struct FilterRegistry {
    constructors: HashMap<String, FilterConstructor>,
}

impl FilterRegistry {
    /// Registers `constructor` under `name`, returning the constructor it replaced.
    pub fn insert(
        &mut self,
        name: impl Into<String>,
        constructor: FilterConstructor,
    ) -> Option<FilterConstructor> {
        self.constructors.insert(name.into(), constructor)
    }

    /// Removes the constructor registered under `name`.
    ///
    /// Active filters of that name keep running until a later
    /// [`Driver::set_filters`] leaves them out.
    ///
    /// [`Driver::set_filters`]: crate::driver::Driver::set_filters
    pub fn remove(&mut self, name: &str) -> Option<FilterConstructor> {
        self.constructors.remove(name)
    }

    /// Returns whether a filter is registered under `name`.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.constructors.contains_key(name)
    }

    /// Returns the registered filter names, in no particular order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.constructors.keys().map(String::as_str)
    }

    pub(crate) fn create(&self, name: &str) -> Option<Result<Box<dyn AudioFilter>, FilterError>> {
        self.constructors.get(name).map(|constructor| constructor())
    }
}

impl Debug for FilterRegistry {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_set().entries(self.names()).finish()
    }
}

/// An audio filter applied to mixed audio before it is encoded.
///
/// Filters are chained in the order given to [`Driver::set_filters`], and the output
/// of one filter is the input of the next. Samples are interleaved `f32`, at 48kHz,
/// with the channel count of the driver's [`MixMode`].
///
/// Filters are run on the mixing thread, and should not block.
///
/// [`Driver::set_filters`]: crate::driver::Driver::set_filters
/// [`MixMode`]: crate::driver::MixMode
pub trait AudioFilter: Send {
    /// Applies a new configuration to this filter.
    ///
    /// This is called when the filter is created, and whenever its configuration is updated.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration is invalid. The previous configuration
    /// is expected to remain active.
    fn configure(&mut self, config: Box<dyn Any + Send>) -> Result<(), FilterError>;

    /// Filters `input` into `output`, returning the number of frames written.
    ///
    /// At most `output.len() / channels` frames may be written. A filter which produces
    /// more audio than fits, such as a time stretch, keeps the rest and returns it on
    /// later calls. An empty `input` asks the filter for audio it is holding, and
    /// returns 0 if there is none.
    ///
    /// `last` marks that no more audio follows `input`, such as when a track ends. The
    /// filter then also returns any audio it was holding back to wait for more input. This
    /// is repeated with an empty `input` until the filter returns 0, which it must
    /// eventually do. Audio given after that is the start of something new.
    ///
    /// # Errors
    ///
    /// Returns an error if the input could not be processed. The input is then passed
    /// on unfiltered.
    fn process(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        channels: usize,
        last: bool,
    ) -> Result<usize, FilterError>;

    /// Drops all held audio and history, such as after a seek or track change.
    fn reset(&mut self);
}
