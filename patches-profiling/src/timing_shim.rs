use std::sync::Arc;
use std::time::Instant;

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor, ModuleShape,
    OutputPort, ParameterMap, PeriodicUpdate,
};
use patches_core::build_error::BuildError;

use crate::timing_collector::TimingCollector;

/// Wraps a `Box<dyn Module>`, delegates every `Module` method to the inner
/// module, and times `process()` and `periodic_update()` calls, recording
/// results into a shared [`TimingCollector`].
pub struct TimingShim {
    inner: Box<dyn Module>,
    collector: Arc<TimingCollector>,
    name: &'static str,
}

impl TimingShim {
    pub fn new(inner: Box<dyn Module>, collector: Arc<TimingCollector>) -> Self {
        let name = inner.descriptor().module_name;
        Self { inner, collector, name }
    }
}

impl Module for TimingShim {
    fn describe(_shape: &ModuleShape) -> ModuleDescriptor
    where
        Self: Sized,
    {
        unimplemented!("TimingShim is a wrapper; construct via TimingShim::new")
    }

    fn prepare(
        _audio_environment: &AudioEnvironment,
        _descriptor: ModuleDescriptor,
        _instance_id: InstanceId,
    ) -> Self
    where
        Self: Sized,
    {
        unimplemented!("TimingShim is a wrapper; construct via TimingShim::new")
    }

    fn update_validated_parameters(&mut self, params: &ParameterMap) {
        self.inner.update_validated_parameters(params);
    }

    fn update_parameters(&mut self, params: &ParameterMap) -> Result<(), BuildError> {
        self.inner.update_parameters(params)
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        self.inner.descriptor()
    }

    fn instance_id(&self) -> InstanceId {
        self.inner.instance_id()
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let t0 = Instant::now();
        self.inner.process(pool);
        let nanos = t0.elapsed().as_nanos() as u64;
        self.collector.record_process(self.inner.instance_id(), self.name, nanos);
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.inner.set_ports(inputs, outputs);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_periodic(&mut self) -> Option<&mut dyn PeriodicUpdate> {
        if self.inner.as_periodic().is_some() {
            Some(self)
        } else {
            None
        }
    }
}

impl PeriodicUpdate for TimingShim {
    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        let t0 = Instant::now();
        self.inner.as_periodic().unwrap().periodic_update(pool);
        let nanos = t0.elapsed().as_nanos() as u64;
        self.collector.record_periodic(self.inner.instance_id(), self.name, nanos);
    }
}
