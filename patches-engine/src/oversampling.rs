/// The oversampling factor applied by the audio engine.
///
/// The engine runs `factor()` inner ticks for every output frame. Modules are
/// initialised with `sample_rate * factor` so that all frequency calculations
/// remain correct at the elevated internal rate. The decimator in
/// `AudioCallback` converts the oversampled output back to the hardware rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OversamplingFactor {
    /// No oversampling: one tick per output frame, sample rate unchanged.
    None,
    /// 2× oversampling: two inner ticks per output frame.
    X2,
    /// 4× oversampling: four inner ticks per output frame.
    X4,
    /// 8× oversampling: eight inner ticks per output frame.
    X8,
}

impl OversamplingFactor {
    /// The integer multiplier corresponding to this variant.
    ///
    /// `None` → 1, `X2` → 2, `X4` → 4, `X8` → 8.
    pub fn factor(self) -> usize {
        match self {
            OversamplingFactor::None => 1,
            OversamplingFactor::X2 => 2,
            OversamplingFactor::X4 => 4,
            OversamplingFactor::X8 => 8,
        }
    }
}
