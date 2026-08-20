use super::{
    AudioStreamConfig, AudioStreamShim, PipeWireCapabilities, StreamBackend, StreamError,
    probe_pipewire,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioRuntimeError {
    PipeWireUnavailable,
    Stream(StreamError),
}

impl From<StreamError> for AudioRuntimeError {
    fn from(value: StreamError) -> Self {
        Self::Stream(value)
    }
}

#[derive(Debug)]
pub struct AudioRuntime {
    capabilities: PipeWireCapabilities,
}

impl AudioRuntime {
    pub fn new() -> Self {
        Self {
            capabilities: probe_pipewire(),
        }
    }

    pub fn capabilities(&self) -> &PipeWireCapabilities {
        &self.capabilities
    }

    pub fn open_stream(
        &self,
        backend: StreamBackend,
        config: AudioStreamConfig,
    ) -> Result<AudioStreamShim, AudioRuntimeError> {
        if !self.capabilities.available || !self.capabilities.has_stream_api {
            return Err(AudioRuntimeError::PipeWireUnavailable);
        }
        Ok(AudioStreamShim::open(backend, config)?)
    }
}

impl Default for AudioRuntime {
    fn default() -> Self {
        Self::new()
    }
}
