use patches_core::{AudioEnvironment, ModuleShape};

/// Trait for modules that can pre-process file contents on the control thread.
///
/// Modules implementing this trait have their `process_file` method called by
/// the planner during plan building, before the execution plan reaches the
/// audio thread. The result is stored as a `ParameterValue::FloatBuffer(Arc<[f32]>)`
/// which the module receives in `update_validated_parameters`.
///
/// `process_file` is a **static method** — it does not require a module instance.
/// It runs on the control thread and may perform file I/O, FFT computation, and
/// other expensive operations.
pub trait FileProcessor {
    /// Read and pre-process a file, returning the processed data as a flat
    /// float buffer.
    ///
    /// # Arguments
    ///
    /// - `env`: audio environment (sample rate, etc.)
    /// - `shape`: the module's shape (may affect processing, e.g. `high_quality`)
    /// - `param_name`: which file parameter is being processed (a module may have
    ///   multiple file parameters)
    /// - `path`: absolute file path (already resolved by the interpreter)
    ///
    /// # Errors
    ///
    /// Returns `Err(message)` if the file cannot be read or processed. The error
    /// propagates through the normal build error path.
    fn process_file(
        env: &AudioEnvironment,
        shape: &ModuleShape,
        param_name: &str,
        path: &str,
    ) -> Result<Vec<f32>, String>
    where
        Self: Sized;
}
