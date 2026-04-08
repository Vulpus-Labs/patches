/// Environmental parameters supplied to modules once when a plan is activated.
///
/// Modules that depend on these parameters (e.g. oscillators that use `sample_rate`)
/// should store them in [`Module::initialise`] and use the stored copies during
/// [`Module::process`] rather than receiving them per sample.
///
/// `poly_voices` is the number of active polyphony voices. Poly cable buffers always
/// hold 16 channels (`[f32; 16]`) regardless of this value; modules should use
/// `poly_voices` to know how many of those channels carry live data.
///
/// `periodic_update_interval` is the number of inner ticks between successive
/// [`PeriodicUpdate::periodic_update`] calls. At 1× oversampling this equals
/// [`BASE_PERIODIC_UPDATE_INTERVAL`] (32); at N× oversampling it equals
/// `BASE_PERIODIC_UPDATE_INTERVAL * N`, preserving the same wall-clock update rate.
/// Modules implementing [`PeriodicUpdate`] should use this value (not the
/// compile-time constant) to compute per-sample interpolation deltas.
///
/// [`BASE_PERIODIC_UPDATE_INTERVAL`]: crate::BASE_PERIODIC_UPDATE_INTERVAL
/// [`PeriodicUpdate`]: crate::PeriodicUpdate
/// [`PeriodicUpdate::periodic_update`]: crate::PeriodicUpdate::periodic_update
#[derive(Debug, Clone, Copy)]
pub struct AudioEnvironment {
    pub sample_rate: f32,
    pub poly_voices: usize,
    pub periodic_update_interval: u32,
}