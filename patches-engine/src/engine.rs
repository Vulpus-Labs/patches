use std::fmt;
use std::sync::Arc;
use std::thread;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};

use patches_core::{AudioEnvironment, CableValue, Module};

use crate::builder::ExecutionPlan;
use crate::callback::{build_stream, AudioCallback};
use crate::execution_state::SUB_BLOCK_SIZE;
use crate::midi::{AudioClock, EventQueueConsumer};
use crate::oversampling::OversamplingFactor;
use crate::pool::ModulePool;

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
    /// Human-readable device name as reported by the OS.
    pub name: String,
    /// `true` if the device supports audio input.
    pub is_input: bool,
    /// `true` if the device supports audio output.
    pub is_output: bool,
}

/// Enumerate all available audio devices on the default CPAL host.
///
/// Returns a list of [`DeviceInfo`] structs describing each device's name
/// and whether it supports input, output, or both.  No streams are opened;
/// this is purely informational and can be called before
/// [`SoundEngine::open`].
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

/// Default module pool capacity: number of `Option<Box<dyn Module>>` slots
/// pre-allocated on the audio thread.
///
/// 1024 was chosen as a round power-of-two that comfortably covers any realistic
/// patch (typical patches use tens of modules; even dense live-coding setups rarely
/// exceed a few hundred).  It supports up to ~1000 simultaneous module instances
/// with a small margin.  If a patch exceeds this limit, increase this constant or
/// supply a custom capacity via [`SoundEngine::with_capacity`].
pub const DEFAULT_MODULE_POOL_CAPACITY: usize = 1024;

/// A value sent to the `"patches-cleanup"` thread for deallocation off the
/// audio thread.
///
/// Introduced in T-0169 to replace the bare `Box<dyn Module>` ring buffer
/// element type. The cleanup thread simply drops whichever variant it receives.
pub enum CleanupAction {
    /// A module evicted from the pool via [`ModulePool::tombstone`].
    DropModule(Box<dyn Module>),
    /// An [`ExecutionPlan`] replaced by a newer one.
    DropPlan(Box<ExecutionPlan>),
}

/// Pre-start state: the plan channel consumer, the cable buffer pool, and the
/// module pool. Stored in [`SoundEngine`] until [`start`](SoundEngine::start)
/// moves them into the audio closure.
struct PendingState {
    plan_rx: rtrb::Consumer<ExecutionPlan>,
    buffer_pool: Box<[[CableValue; 2]]>,
    module_pool: ModulePool,
}

/// State captured by [`SoundEngine::open`]: the audio device, its stream
/// configuration, sample format, and channel count. Held until
/// [`start`](SoundEngine::start) uses them to build the output stream.
struct OpenedDevice {
    device: Device,
    config: StreamConfig,
    sample_format: SampleFormat,
    channels: usize,
}

/// Errors returned by [`SoundEngine`] operations.
#[derive(Debug)]
pub enum EngineError {
    /// No default output device is available on this system.
    NoOutputDevice,
    /// The requested device name was not found among available devices.
    DeviceNotFound(String),
    /// Failed to query the device's default stream configuration.
    DefaultConfigError(cpal::DefaultStreamConfigError),
    /// Failed to build the output stream.
    BuildStreamError(cpal::BuildStreamError),
    /// Failed to start stream playback.
    PlayStreamError(cpal::PlayStreamError),
    /// The device's native sample format is not supported by this engine.
    UnsupportedSampleFormat(SampleFormat),
    /// [`start`](SoundEngine::start) was called a second time after the engine
    /// has already been started and stopped. Create a new [`SoundEngine`] to
    /// restart with a fresh plan.
    AlreadyConsumed,
    /// The OS refused to spawn the cleanup thread.
    ThreadSpawnError(std::io::Error),
    /// [`start`](SoundEngine::start) was called before [`open`](SoundEngine::open).
    NotOpened,
    /// [`open`](SoundEngine::open) was called after the device has already been opened.
    AlreadyOpened,
    /// Failed to open the WAV recording file.
    RecordOpenError(std::io::Error),
    /// The input device's native sample rate does not match the output device.
    SampleRateMismatch { input: u32, output: u32 },
    /// Failed to query available devices from the host.
    DevicesError(cpal::DevicesError),
    /// Failed to build the input stream.
    BuildInputStreamError(cpal::BuildStreamError),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::NoOutputDevice => write!(f, "no default output device available"),
            EngineError::DeviceNotFound(name) => {
                write!(f, "audio device not found: {name:?}")
            }
            EngineError::DefaultConfigError(e) => {
                write!(f, "failed to get device config: {e}")
            }
            EngineError::BuildStreamError(e) => write!(f, "failed to build stream: {e}"),
            EngineError::PlayStreamError(e) => write!(f, "failed to play stream: {e}"),
            EngineError::UnsupportedSampleFormat(fmt) => {
                write!(f, "unsupported sample format: {fmt:?}")
            }
            EngineError::AlreadyConsumed => write!(
                f,
                "engine has already been started and stopped; create a new SoundEngine to restart"
            ),
            EngineError::ThreadSpawnError(e) => write!(f, "failed to spawn cleanup thread: {e}"),
            EngineError::NotOpened => {
                write!(f, "start() called before open(); call open() first")
            }
            EngineError::AlreadyOpened => {
                write!(f, "open() called after the device has already been opened")
            }
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

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::sync::{Arc, Mutex};

    use patches_core::{AudioEnvironment, InstanceId, Module, ModuleDescriptor, ModuleShape};
    use patches_core::parameter_map::ParameterMap;

    use super::CleanupAction;

    struct ThreadIdDropSpy {
        instance_id: InstanceId,
        descriptor: ModuleDescriptor,
        drop_thread: Arc<Mutex<Option<String>>>,
    }

    impl ThreadIdDropSpy {
        fn new(drop_thread: Arc<Mutex<Option<String>>>) -> Self {
            Self {
                instance_id: InstanceId::next(),
                descriptor: ModuleDescriptor {
                    module_name: "ThreadIdDropSpy",
                    shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
                    inputs: vec![],
                    outputs: vec![],
                    parameters: vec![],
                },
                drop_thread,
            }
        }
    }

    impl Drop for ThreadIdDropSpy {
        fn drop(&mut self) {
            let name = std::thread::current().name().map(str::to_owned);
            *self.drop_thread.lock().unwrap() = name;
        }
    }

    impl Module for ThreadIdDropSpy {
        fn describe(_shape: &ModuleShape) -> ModuleDescriptor {
            ModuleDescriptor {
                module_name: "ThreadIdDropSpy",
                shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
                inputs: vec![],
                outputs: vec![],
                parameters: vec![],
            }
        }

        fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
            Self {
                instance_id,
                descriptor,
                drop_thread: Arc::new(Mutex::new(None)),
            }
        }

        fn update_validated_parameters(&mut self, _params: &mut ParameterMap) {}

        fn descriptor(&self) -> &ModuleDescriptor {
            &self.descriptor
        }

        fn instance_id(&self) -> InstanceId {
            self.instance_id
        }

        fn process(&mut self, _pool: &mut patches_core::CablePool<'_>) {}

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    /// The cleanup channel and thread can be exercised in isolation without CPAL.
    ///
    /// A `ThreadIdDropSpy` wrapped in `CleanupAction::DropModule` must be dropped
    /// on the thread named `"patches-cleanup"`, not on the test thread.
    #[test]
    fn tombstoned_module_dropped_on_cleanup_thread() {
        let drop_thread: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let spy = Box::new(ThreadIdDropSpy::new(Arc::clone(&drop_thread)));
        let (mut tx, rx) = rtrb::RingBuffer::<CleanupAction>::new(16);

        let handle = crate::kernel::spawn_cleanup_thread(rx).unwrap();

        tx.push(CleanupAction::DropModule(spy)).unwrap();
        drop(tx); // abandon producer → cleanup thread exits
        handle.join().unwrap();

        let recorded = drop_thread.lock().unwrap().clone();
        assert_eq!(recorded, Some("patches-cleanup".to_owned()));
    }
}

/// Drives an [`ExecutionPlan`] continuously, writing stereo output to the
/// default hardware audio device via CPAL.
///
/// The audio callback owns the module pool and cable buffer pool directly -- no
/// `Arc`, no `Mutex`. A new plan can be swapped in at any time via
/// [`swap_plan`](Self::swap_plan), which sends it over a wait-free SPSC
/// channel (`rtrb`).
///
/// When the audio callback adopts a new plan it:
///   1. Takes tombstoned modules out of the pool and sends them to the cleanup
///      ring buffer so they are deallocated off the audio thread. If the ring
///      buffer is full the module is dropped inline with a diagnostic message.
///   2. Installs `new_modules` into the module pool.
///   3. Zeros `to_zero` cable buffer slots.
///   4. Replaces `current_plan`.
///
/// Modules must be fully constructed and initialised before being sent to the
/// engine. The engine does **not** call [`Module::initialise`]; callers are
/// responsible for initialising modules with the [`AudioEnvironment`] returned
/// by [`open`](Self::open) before passing plans to [`start`](Self::start) or
/// [`swap_plan`](Self::swap_plan).
///
/// ## Lifecycle
///
/// 1. [`new`](Self::new) -- create the engine (no plan, no device).
/// 2. [`open`](Self::open) -- open the audio device, query the sample rate,
///    return an [`AudioEnvironment`]. Does **not** start audio.
/// 3. [`start`](Self::start) -- take a fully-constructed [`ExecutionPlan`]
///    and begin audio processing.
/// 4. [`swap_plan`](Self::swap_plan) -- hot-swap to a new plan at any time.
/// 5. [`stop`](Self::stop) -- stop audio and release the device.
///
/// A `SoundEngine` can be started once. After [`stop`](Self::stop) the plan
/// has been moved into (and dropped with) the audio closure; to run again
/// with a new plan, create a fresh `SoundEngine`.
pub struct SoundEngine {
    /// Write end of the lock-free plan channel.
    plan_tx: rtrb::Producer<ExecutionPlan>,
    /// Consumer end and pools -- stashed here until
    /// [`start`](Self::start) moves them into the audio closure.
    /// `None` after `start()` has been called.
    pending: Option<PendingState>,
    /// Device state captured by [`open`](Self::open), consumed by
    /// [`start`](Self::start).
    opened_device: Option<OpenedDevice>,
    /// Live CPAL stream while the engine is running.
    stream: Option<Stream>,
    /// Capacity of the module pool; used to size the cleanup ring buffer.
    module_pool_capacity: usize,
    /// Join handle for the cleanup thread spawned in [`start`](Self::start).
    cleanup_thread: Option<thread::JoinHandle<()>>,
    /// Shared audio clock. The audio callback holds a raw pointer into this
    /// allocation; `SoundEngine` keeps the `Arc` alive so the pointer remains
    /// valid for the lifetime of the stream.
    clock: Arc<AudioClock>,
    /// Optional WAV recorder.  Held here so that `stop()` can signal the
    /// writer thread *after* dropping the CPAL stream (ensuring no more frames
    /// are pushed to the ring buffer before the writer drains and finalises).
    recorder: Option<crate::wav_recorder::WavRecorder>,
    /// Input capture handle.  Holds the CPAL input stream alive.
    /// Dropped by [`stop`](Self::stop) after the output stream.
    input_capture: Option<crate::input_capture::InputCapture>,
    /// Ring buffer consumer for input audio frames.  Moved into
    /// [`AudioCallback`] by [`start`](Self::start).
    input_rx: Option<rtrb::Consumer<[f32; 2]>>,
    /// Number of inner ticks executed per output frame.  Stored so that
    /// [`open`](Self::open) can multiply the device sample rate by this factor
    /// when constructing the [`AudioEnvironment`], and [`start`](Self::start)
    /// can pass it to [`AudioCallback`].
    oversampling_factor: usize,
    /// Base control period in output frames (default: `SUB_BLOCK_SIZE`).
    /// Multiplied by `oversampling_factor` inside [`AudioCallback`] to give
    /// the inner-tick count per MIDI sub-block, preserving the wall-clock
    /// control rate.  Set via `PatchEngine::with_control_period`.
    pub(crate) control_period: usize,
}

impl SoundEngine {
    /// Create a new `SoundEngine` with pre-allocated pools but no plan and no
    /// audio device.
    ///
    /// `buffer_pool_capacity` is the number of `[f32; 2]` cable buffer slots.
    /// Slot 0 is the permanent-zero slot; slots 1... are for cable buffers.
    ///
    /// `module_pool_capacity` is the number of `Option<Box<dyn Module>>` slots
    /// in the audio-thread module pool. Must be at least as large as the value
    /// used when building plans via [`build_patch`](crate::build_patch).
    ///
    /// No audio device is opened until [`open`](Self::open) is called.
    pub fn new(
        buffer_pool_capacity: usize,
        module_pool_capacity: usize,
        oversampling: OversamplingFactor,
    ) -> Result<Self, EngineError> {
        let buffer_pool = crate::kernel::init_buffer_pool(buffer_pool_capacity);
        let module_pool = ModulePool::new(module_pool_capacity);
        let (plan_tx, plan_rx) = rtrb::RingBuffer::new(1);
        let clock = Arc::new(AudioClock::new());
        Ok(Self {
            plan_tx,
            pending: Some(PendingState { plan_rx, buffer_pool, module_pool }),
            opened_device: None,
            stream: None,
            module_pool_capacity,
            cleanup_thread: None,
            clock,
            recorder: None,
            input_capture: None,
            input_rx: None,
            oversampling_factor: oversampling.factor(),
            control_period: SUB_BLOCK_SIZE as usize,
        })
    }

    /// Open audio device(s) according to `device_config` and query their
    /// configuration.
    ///
    /// Returns an [`AudioEnvironment`] containing the (oversampled) sample
    /// rate.  The output device and configuration are stored internally for use
    /// by [`start`](Self::start).  If `device_config.input_device` is `Some`,
    /// the input device is also opened and validated (sample rate must match
    /// the output device).
    ///
    /// Does **not** start the audio thread.
    ///
    /// Returns [`EngineError::AlreadyOpened`] if called a second time.
    pub fn open(&mut self, device_config: &DeviceConfig) -> Result<AudioEnvironment, EngineError> {
        if self.opened_device.is_some() {
            return Err(EngineError::AlreadyOpened);
        }

        let host = cpal::default_host();

        // ── Output device ────────────────────────────────────────────────
        let device = match &device_config.output_device {
            Some(name) => host
                .output_devices()
                .map_err(EngineError::DevicesError)?
                .find(|d| d.name().is_ok_and(|n| n == *name))
                .ok_or_else(|| EngineError::DeviceNotFound(name.clone()))?,
            None => host
                .default_output_device()
                .ok_or(EngineError::NoOutputDevice)?,
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

        self.opened_device = Some(OpenedDevice {
            device,
            config,
            sample_format,
            channels,
        });

        // ── Input device (optional) ──────────────────────────────────────
        if let Some(input_name) = &device_config.input_device {
            let input_device = host
                .input_devices()
                .map_err(EngineError::DevicesError)?
                .find(|d| d.name().is_ok_and(|n| n == *input_name))
                .ok_or_else(|| EngineError::DeviceNotFound(input_name.clone()))?;

            let (capture, rx) = crate::input_capture::open_input(
                &input_device,
                output_rate,
            )?;
            self.input_capture = Some(capture);
            self.input_rx = Some(rx);
        }

        Ok(AudioEnvironment {
            sample_rate,
            poly_voices: 16,
            periodic_update_interval: patches_core::BASE_PERIODIC_UPDATE_INTERVAL * self.oversampling_factor as u32,
            hosted: false,
        })
    }

    /// Begin audio processing with the given [`ExecutionPlan`].
    ///
    /// The plan's modules must already be fully constructed and initialised
    /// (e.g. via [`Module::initialise`] with the [`AudioEnvironment`] returned
    /// by [`open`](Self::open)). The engine installs `new_modules` into the
    /// module pool and starts the audio callback.
    ///
    /// Returns [`EngineError::NotOpened`] if [`open`](Self::open) has not been
    /// called. Returns [`EngineError::AlreadyConsumed`] if the engine has
    /// already been started and stopped.
    pub fn start(
        &mut self,
        event_queue: Option<EventQueueConsumer>,
        record_path: Option<&str>,
    ) -> Result<(), EngineError> {
        if self.stream.is_some() {
            return Ok(());
        }

        let PendingState { plan_rx, buffer_pool, module_pool } =
            self.pending.take().ok_or(EngineError::AlreadyConsumed)?;

        let OpenedDevice { device, config, sample_format, channels } =
            self.opened_device.take().ok_or(EngineError::NotOpened)?;

        let (cleanup_tx, cleanup_rx) =
            rtrb::RingBuffer::<CleanupAction>::new(self.module_pool_capacity);

        let cleanup_handle = crate::kernel::spawn_cleanup_thread(cleanup_rx)
            .map_err(EngineError::ThreadSpawnError)?;

        self.cleanup_thread = Some(cleanup_handle);

        let record_tx = if let Some(path) = record_path {
            let sample_rate = config.sample_rate.0;
            let (recorder, tx) = crate::wav_recorder::open(path, sample_rate)
                .map_err(EngineError::RecordOpenError)?;
            self.recorder = Some(recorder);
            Some(tx)
        } else {
            None
        };

        // Start the input stream if one was opened.
        if let Some(ref capture) = self.input_capture {
            capture.play().map_err(EngineError::PlayStreamError)?;
        }

        let processor = crate::processor::PatchProcessor::from_parts(
            buffer_pool,
            module_pool,
            self.oversampling_factor,
            cleanup_tx,
        );

        // Pass a raw pointer into the Arc's allocation. The Arc in `self.clock`
        // keeps the AudioClock alive for at least as long as `self` exists, and
        // `stop()` drops the stream (and thus the callback) before `self` is
        // dropped, so the pointer is valid for the entire lifetime of the callback.
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

    /// Stop audio processing and close the device.
    ///
    /// Drops the CPAL stream first (which drops the audio callback and its
    /// `cleanup_tx` producer, signalling the cleanup thread to exit), then
    /// joins the cleanup thread so all tombstoned modules are guaranteed to
    /// have been dropped before this method returns.
    ///
    /// Idempotent: safe to call multiple times or if the engine was never started.
    pub fn stop(&mut self) {
        // Drop the CPAL output stream first — this stops the audio callback and
        // drops the `record_tx` producer, so no more frames will be pushed.
        self.stream.take();
        // Drop the input capture (stops the CPAL input stream).
        self.input_capture.take();
        // Then stop the WAV recorder: it drains remaining buffered frames,
        // finalises the file, and joins the writer thread.
        self.recorder.take();
        if let Some(handle) = self.cleanup_thread.take() {
            let _ = handle.join();
        }
    }

    /// Send a new [`ExecutionPlan`] to the audio callback.
    ///
    /// The plan's modules must already be fully constructed and initialised.
    /// The engine does **not** call [`Module::initialise`] on incoming modules.
    ///
    /// The callback will adopt the new plan at the start of its next invocation,
    /// installing new modules and tombstoning removed ones. If the single-slot
    /// channel is already full, the push is a no-op and `new_plan` is returned
    /// as `Err`. Callers may retry; the audio callback drains the slot within
    /// one buffer period (~10 ms).
    ///
    /// This method is wait-free and safe to call from any thread.
    // ExecutionPlan must be returned as-is on error (for retry semantics); boxing
    // would require an allocation, violating the wait-free contract.
    #[allow(clippy::result_large_err)]
    pub fn swap_plan(&mut self, new_plan: ExecutionPlan) -> Result<(), ExecutionPlan> {
        self.plan_tx.push(new_plan).map_err(|rtrb::PushError::Full(v)| v)
    }

    /// Return a clone of the shared [`AudioClock`].
    ///
    /// MIDI connector threads call [`AudioClock::read`] on this to convert
    /// wall-clock event timestamps to sample positions.
    pub fn clock(&self) -> Arc<AudioClock> {
        Arc::clone(&self.clock)
    }
}
