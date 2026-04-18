use std::fmt;
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};

use patches_core::AudioEnvironment;
use patches_planner::ExecutionPlan;
use patches_engine::execution_state::SUB_BLOCK_SIZE;
use patches_engine::midi::{AudioClock, EventQueueConsumer};
use patches_engine::oversampling::OversamplingFactor;
use patches_engine::processor::PatchProcessor;

use crate::callback::{build_stream, AudioCallback};

/// Configuration for audio device selection.
///
/// When a field is `None`, the system default is used.  Setting
/// `input_device` to `None` disables audio input capture entirely.
#[derive(Debug, Clone, Default)]
pub struct DeviceConfig {
    /// Output device name.  `None` = system default output.
    pub output_device: Option<String>,
    /// Input device name.  `None` = no input capture.  `Some(name)` opens the
    /// named device for audio input.
    pub input_device: Option<String>,
}

/// Description of an available audio device returned by [`enumerate_devices`].
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub is_input: bool,
    pub is_output: bool,
}

/// Enumerate all available audio devices on the default CPAL host.
pub fn enumerate_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let mut seen = std::collections::HashMap::<String, (bool, bool)>::new();

    if let Ok(outputs) = host.output_devices() {
        for d in outputs {
            if let Ok(name) = d.name() {
                seen.entry(name).or_default().1 = true;
            }
        }
    }
    if let Ok(inputs) = host.input_devices() {
        for d in inputs {
            if let Ok(name) = d.name() {
                seen.entry(name).or_default().0 = true;
            }
        }
    }

    let mut devices: Vec<DeviceInfo> = seen
        .into_iter()
        .map(|(name, (is_input, is_output))| DeviceInfo { name, is_input, is_output })
        .collect();
    devices.sort_by(|a, b| a.name.cmp(&b.name));
    devices
}

pub use patches_engine::DEFAULT_MODULE_POOL_CAPACITY;

/// State captured by [`SoundEngine::open`]: device + stream config.
struct OpenedDevice {
    device: Device,
    config: StreamConfig,
    sample_format: SampleFormat,
    channels: usize,
}

/// Errors returned by [`SoundEngine`] operations.
#[derive(Debug)]
pub enum EngineError {
    NoOutputDevice,
    DeviceNotFound(String),
    DefaultConfigError(cpal::DefaultStreamConfigError),
    BuildStreamError(cpal::BuildStreamError),
    PlayStreamError(cpal::PlayStreamError),
    UnsupportedSampleFormat(SampleFormat),
    NotOpened,
    AlreadyOpened,
    AlreadyStarted,
    RecordOpenError(std::io::Error),
    SampleRateMismatch { input: u32, output: u32 },
    DevicesError(cpal::DevicesError),
    BuildInputStreamError(cpal::BuildStreamError),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::NoOutputDevice => write!(f, "no default output device available"),
            EngineError::DeviceNotFound(name) => write!(f, "audio device not found: {name:?}"),
            EngineError::DefaultConfigError(e) => write!(f, "failed to get device config: {e}"),
            EngineError::BuildStreamError(e) => write!(f, "failed to build stream: {e}"),
            EngineError::PlayStreamError(e) => write!(f, "failed to play stream: {e}"),
            EngineError::UnsupportedSampleFormat(fmt) => write!(f, "unsupported sample format: {fmt:?}"),
            EngineError::NotOpened => write!(f, "start() called before open(); call open() first"),
            EngineError::AlreadyOpened => write!(f, "open() called after the device has already been opened"),
            EngineError::AlreadyStarted => write!(f, "start() called after the engine has already been started"),
            EngineError::RecordOpenError(e) => write!(f, "failed to open WAV recording file: {e}"),
            EngineError::SampleRateMismatch { input, output } => {
                write!(f, "input device sample rate ({input} Hz) does not match output device ({output} Hz)")
            }
            EngineError::DevicesError(e) => write!(f, "failed to query audio devices: {e}"),
            EngineError::BuildInputStreamError(e) => write!(f, "failed to build input stream: {e}"),
        }
    }
}

impl std::error::Error for EngineError {}

/// Drives an externally-supplied [`PatchProcessor`] against a CPAL output
/// stream.
///
/// The processor and plan-channel consumer are built by the host (via
/// `patches-host::HostBuilder`) and handed to [`start`](Self::start).
/// `SoundEngine` owns only CPAL device state, the audio clock, optional
/// WAV recording, and optional audio input capture.
///
/// ## Lifecycle
///
/// 1. [`new`](Self::new) — create the engine (no device).
/// 2. [`open`](Self::open) — open the device and return its
///    [`AudioEnvironment`] so the caller can build a `HostRuntime` against
///    the actual sample rate.
/// 3. [`start`](Self::start) — install the processor + plan consumer and
///    begin audio.
/// 4. [`stop`](Self::stop) — release the stream and joint optional
///    recorder.
pub struct SoundEngine {
    opened_device: Option<OpenedDevice>,
    stream: Option<Stream>,
    clock: Arc<AudioClock>,
    recorder: Option<patches_io::wav_recorder::WavRecorder>,
    input_capture: Option<crate::input_capture::InputCapture>,
    input_rx: Option<rtrb::Consumer<[f32; 2]>>,
    oversampling_factor: usize,
    /// Base control period in output frames (default: `SUB_BLOCK_SIZE`).
    pub control_period: usize,
}

impl SoundEngine {
    /// Create a new `SoundEngine`. No device is opened until [`open`] is called.
    pub fn new(oversampling: OversamplingFactor) -> Self {
        Self {
            opened_device: None,
            stream: None,
            clock: Arc::new(AudioClock::new()),
            recorder: None,
            input_capture: None,
            input_rx: None,
            oversampling_factor: oversampling.factor(),
            control_period: SUB_BLOCK_SIZE as usize,
        }
    }

    /// Open audio device(s) according to `device_config`.
    pub fn open(&mut self, device_config: &DeviceConfig) -> Result<AudioEnvironment, EngineError> {
        if self.opened_device.is_some() {
            return Err(EngineError::AlreadyOpened);
        }

        let host = cpal::default_host();

        let device = match &device_config.output_device {
            Some(name) => host
                .output_devices()
                .map_err(EngineError::DevicesError)?
                .find(|d| d.name().is_ok_and(|n| n == *name))
                .ok_or_else(|| EngineError::DeviceNotFound(name.clone()))?,
            None => host.default_output_device().ok_or(EngineError::NoOutputDevice)?,
        };

        let supported = device
            .default_output_config()
            .map_err(EngineError::DefaultConfigError)?;

        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let output_rate = config.sample_rate.0;
        let device_rate = f64::from(output_rate) as f32;
        let sample_rate = device_rate * self.oversampling_factor as f32;
        let channels = usize::from(config.channels);

        self.opened_device = Some(OpenedDevice { device, config, sample_format, channels });

        if let Some(input_name) = &device_config.input_device {
            let input_device = host
                .input_devices()
                .map_err(EngineError::DevicesError)?
                .find(|d| d.name().is_ok_and(|n| n == *input_name))
                .ok_or_else(|| EngineError::DeviceNotFound(input_name.clone()))?;

            let (capture, rx) = crate::input_capture::open_input(&input_device, output_rate)?;
            self.input_capture = Some(capture);
            self.input_rx = Some(rx);
        }

        Ok(AudioEnvironment {
            sample_rate,
            poly_voices: 16,
            periodic_update_interval: patches_core::BASE_PERIODIC_UPDATE_INTERVAL
                * self.oversampling_factor as u32,
            hosted: false,
        })
    }

    /// Begin audio processing with an externally-supplied processor and plan
    /// consumer.
    ///
    /// The host (typically `patches-host::HostRuntime`) owns the cleanup thread
    /// and plan producer; this method is purely a CPAL-stream installer.
    pub fn start(
        &mut self,
        processor: PatchProcessor,
        plan_rx: rtrb::Consumer<ExecutionPlan>,
        event_queue: Option<EventQueueConsumer>,
        record_path: Option<&str>,
    ) -> Result<(), EngineError> {
        if self.stream.is_some() {
            return Err(EngineError::AlreadyStarted);
        }

        let OpenedDevice { device, config, sample_format, channels } =
            self.opened_device.take().ok_or(EngineError::NotOpened)?;

        let record_tx = if let Some(path) = record_path {
            let sample_rate = config.sample_rate.0;
            let (recorder, tx) = patches_io::wav_recorder::open(path, sample_rate)
                .map_err(EngineError::RecordOpenError)?;
            self.recorder = Some(recorder);
            Some(tx)
        } else {
            None
        };

        if let Some(ref capture) = self.input_capture {
            capture.play().map_err(EngineError::PlayStreamError)?;
        }

        // SAFETY: the Arc in `self.clock` outlives the stream; `stop()` drops
        // the stream before `self` is dropped, so the pointer remains valid
        // for the entire lifetime of the callback.
        let clock_ptr = Arc::as_ptr(&self.clock);
        let input_rx = self.input_rx.take();
        let callback = AudioCallback::new(
            plan_rx, processor, channels,
            event_queue, clock_ptr, record_tx,
            self.oversampling_factor,
            self.control_period,
            input_rx,
        );
        let stream = match sample_format {
            SampleFormat::F32 => build_stream::<f32>(&device, &config, callback),
            SampleFormat::I16 => build_stream::<i16>(&device, &config, callback),
            SampleFormat::U16 => build_stream::<u16>(&device, &config, callback),
            other => return Err(EngineError::UnsupportedSampleFormat(other)),
        }?;

        stream.play().map_err(EngineError::PlayStreamError)?;
        self.stream = Some(stream);
        Ok(())
    }

    /// Stop audio processing and release the device.
    pub fn stop(&mut self) {
        self.stream.take();
        self.input_capture.take();
        self.recorder.take();
    }

    /// Return a clone of the shared [`AudioClock`].
    pub fn clock(&self) -> Arc<AudioClock> {
        Arc::clone(&self.clock)
    }
}
