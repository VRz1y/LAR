use super::{DmaBufError, GraphicBuffer, PixelFormat};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub color: [u8; 4],
}

#[derive(Debug, Default, Clone)]
pub struct CpuOverlayManager {
    rectangles: Vec<OverlayRect>,
}
impl CpuOverlayManager {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add(&mut self, rect: OverlayRect) -> Result<(), OverlayError> {
        if rect.width == 0 || rect.height == 0 {
            return Err(OverlayError::InvalidRect);
        }
        self.rectangles.push(rect);
        Ok(())
    }
    pub fn clear(&mut self) {
        self.rectangles.clear();
    }
    pub fn rectangles(&self) -> &[OverlayRect] {
        &self.rectangles
    }
    pub fn apply(&self, buffer: &GraphicBuffer) -> Result<usize, OverlayError> {
        let description = buffer.description();
        let format = match description.format {
            PixelFormat::Rgba8888 | PixelFormat::Bgra8888 => description.format,
            _ => return Err(OverlayError::UnsupportedFormat),
        };
        let plane = buffer.planes().first().ok_or(OverlayError::InvalidBuffer)?;
        let mut mapping = plane.mmap_writable().map_err(OverlayError::DmaBuf)?;
        let mut applied = 0;
        for rect in &self.rectangles {
            let x_end = rect.x.saturating_add(rect.width).min(description.width);
            let y_end = rect.y.saturating_add(rect.height).min(description.height);
            for y in rect.y.min(description.height)..y_end {
                for x in rect.x.min(description.width)..x_end {
                    let offset = y as usize * description.stride as usize * 4 + x as usize * 4;
                    if offset + 4 > mapping.len() {
                        return Err(OverlayError::InvalidBuffer);
                    }
                    let dst = &mut mapping[offset..offset + 4];
                    let alpha = rect.color[3] as u16;
                    let inv = 255 - alpha;
                    let (r, g, b) = (rect.color[0], rect.color[1], rect.color[2]);
                    let (dr, dg, db) = if format == PixelFormat::Bgra8888 {
                        (dst[2], dst[1], dst[0])
                    } else {
                        (dst[0], dst[1], dst[2])
                    };
                    let blended = [
                        (r as u16 * alpha + dr as u16 * inv) / 255,
                        (g as u16 * alpha + dg as u16 * inv) / 255,
                        (b as u16 * alpha + db as u16 * inv) / 255,
                    ];
                    if format == PixelFormat::Bgra8888 {
                        dst[0] = blended[2] as u8;
                        dst[1] = blended[1] as u8;
                        dst[2] = blended[0] as u8;
                    } else {
                        dst[0] = blended[0] as u8;
                        dst[1] = blended[1] as u8;
                        dst[2] = blended[2] as u8;
                    }
                    dst[3] = 255;
                }
            }
            applied += 1;
        }
        Ok(applied)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayError {
    InvalidRect,
    InvalidBuffer,
    UnsupportedFormat,
    DmaBuf(DmaBufError),
}
impl fmt::Display for OverlayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CPU overlay error: {self:?}")
    }
}
impl std::error::Error for OverlayError {}
