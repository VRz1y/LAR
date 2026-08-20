use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Rgba8888,
    Bgra8888,
    Rgb565,
    Nv12,
    Yv12,
    Unknown(u32),
}

impl PixelFormat {
    pub const fn bytes_per_pixel(self) -> Option<usize> {
        match self {
            Self::Rgba8888 | Self::Bgra8888 => Some(4),
            Self::Rgb565 => Some(2),
            Self::Nv12 | Self::Yv12 | Self::Unknown(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferDescription {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: PixelFormat,
}

impl BufferDescription {
    pub fn validate(&self) -> Result<(), BufferError> {
        if self.width == 0 || self.height == 0 || self.stride < self.width {
            return Err(BufferError::InvalidDescription);
        }
        if let Some(bytes) = self.format.bytes_per_pixel() {
            let required = self.stride as usize * bytes;
            if required / bytes != self.stride as usize {
                return Err(BufferError::SizeOverflow);
            }
        }
        Ok(())
    }

    pub fn linear_size(&self) -> Option<usize> {
        let bytes = self.format.bytes_per_pixel()?;
        (self.stride as usize)
            .checked_mul(self.height as usize)
            .and_then(|rows| rows.checked_mul(bytes))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BufferError {
    InvalidDescription,
    SizeOverflow,
    InvalidFd,
    InvalidPlane,
}

impl fmt::Display for BufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDescription => write!(f, "invalid graphics buffer description"),
            Self::SizeOverflow => write!(f, "graphics buffer size overflow"),
            Self::InvalidFd => write!(f, "invalid graphics buffer file descriptor"),
            Self::InvalidPlane => write!(f, "invalid graphics buffer plane"),
        }
    }
}

impl std::error::Error for BufferError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicBuffer {
    description: BufferDescription,
    planes: Vec<crate::graphics::dmabuf::DmaBufPlane>,
}

impl GraphicBuffer {
    pub fn new(
        description: BufferDescription,
        planes: Vec<crate::graphics::dmabuf::DmaBufPlane>,
    ) -> Result<Self, BufferError> {
        description.validate()?;
        if planes.is_empty() {
            return Err(BufferError::InvalidPlane);
        }
        if planes.iter().any(|plane| plane.validate().is_err()) {
            return Err(BufferError::InvalidPlane);
        }
        Ok(Self {
            description,
            planes,
        })
    }

    pub fn description(&self) -> BufferDescription {
        self.description
    }
    pub fn planes(&self) -> &[crate::graphics::dmabuf::DmaBufPlane] {
        &self.planes
    }
}
