use super::pipewire::{PipeWireError, PipeWireStream};
use super::ring_buffer::{MutexRingBuffer, RingBufferError, SpscRingBuffer};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamBackend {
    OpenSlEs,
    AAudio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamDirection {
    Input,
    Output,
    Duplex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioStreamConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub frames_per_buffer: usize,
    pub direction: StreamDirection,
}

impl AudioStreamConfig {
    pub fn validate(self) -> Result<Self, StreamError> {
        if self.sample_rate == 0 || self.channels == 0 || self.frames_per_buffer == 0 {
            return Err(StreamError::InvalidConfig);
        }
        Ok(self)
    }
}

pub struct AudioStreamShim {
    backend: StreamBackend,
    config: AudioStreamConfig,
    input: MutexRingBuffer<f32>,
    output: SpscRingBuffer<f32>,
    state: AtomicU8,
    host: Option<PipeWireStream>,
}

impl AudioStreamShim {
    pub fn open(backend: StreamBackend, config: AudioStreamConfig) -> Result<Self, StreamError> {
        let config = config.validate()?;
        let samples = config
            .frames_per_buffer
            .checked_mul(config.channels as usize)
            .and_then(|value| value.checked_mul(4))
            .ok_or(StreamError::InvalidConfig)?;
        Ok(Self {
            backend,
            config,
            input: MutexRingBuffer::new(samples)?,
            output: SpscRingBuffer::new(samples)?,
            state: AtomicU8::new(0),
            host: None,
        })
    }

    pub fn open_pipewire(
        backend: StreamBackend,
        config: AudioStreamConfig,
    ) -> Result<Self, StreamError> {
        let mut stream = Self::open(backend, config)?;
        stream.host = Some(PipeWireStream::connect(
            "LAR Android audio",
            config.direction != StreamDirection::Input,
        )?);
        Ok(stream)
    }

    pub fn backend(&self) -> StreamBackend {
        self.backend
    }

    pub fn config(&self) -> AudioStreamConfig {
        self.config
    }

    pub fn start(&self) -> Result<(), StreamError> {
        if self
            .state
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(StreamError::AlreadyRunning);
        }
        if let Some(host) = &self.host {
            host.set_active(true)?;
        }
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(host) = &self.host {
            let _ = host.set_active(false);
        }
        self.state.store(0, Ordering::Release);
    }

    pub fn is_running(&self) -> bool {
        self.state.load(Ordering::Acquire) == 1
    }

    pub fn write_output(&self, samples: &[f32]) -> usize {
        samples
            .iter()
            .take_while(|sample| self.output.push(**sample).is_ok())
            .count()
    }

    pub fn read_output(&self, samples: &mut [f32]) -> usize {
        let mut count = 0;
        for sample in samples {
            match self.output.pop() {
                Some(value) => *sample = value,
                None => break,
            }
            count += 1;
        }
        count
    }

    pub fn push_input(&self, samples: &[f32]) -> usize {
        samples
            .iter()
            .take_while(|sample| self.input.push(**sample).is_ok())
            .count()
    }

    pub fn read_input(&self, samples: &mut [f32]) -> usize {
        let mut count = 0;
        for sample in samples {
            match self.input.pop() {
                Some(value) => *sample = value,
                None => break,
            }
            count += 1;
        }
        count
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamError {
    InvalidConfig,
    AlreadyRunning,
    PipeWire(PipeWireError),
    RingBuffer(RingBufferError),
}

impl From<PipeWireError> for StreamError {
    fn from(value: PipeWireError) -> Self {
        Self::PipeWire(value)
    }
}

impl From<RingBufferError> for StreamError {
    fn from(value: RingBufferError) -> Self {
        Self::RingBuffer(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AudioStreamConfig {
        AudioStreamConfig {
            sample_rate: 48_000,
            channels: 2,
            frames_per_buffer: 8,
            direction: StreamDirection::Duplex,
        }
    }

    #[test]
    fn stream_moves_samples_between_endpoints() {
        let stream = AudioStreamShim::open(StreamBackend::AAudio, config()).unwrap();
        assert!(!stream.is_running());
        stream.start().unwrap();
        assert!(stream.is_running());

        let input = [0.25, -0.5, 1.0];
        let mut output = [0.0; 3];
        assert_eq!(stream.write_output(&input), 3);
        assert_eq!(stream.read_output(&mut output), 3);
        assert_eq!(input, output);
        stream.stop();
        assert!(!stream.is_running());
    }

    #[test]
    fn invalid_config_is_rejected() {
        let config = AudioStreamConfig {
            sample_rate: 0,
            ..config()
        };
        assert!(matches!(
            AudioStreamShim::open(StreamBackend::OpenSlEs, config),
            Err(StreamError::InvalidConfig)
        ));
    }
}
