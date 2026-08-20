//! Read-only shadow copy of executable ELF PT_LOAD segments.

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShadowSegment {
    start: usize,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShadowText {
    segments: Vec<ShadowSegment>,
}

impl ShadowText {
    pub fn new(segments: Vec<(usize, Vec<u8>)>) -> Self {
        Self {
            segments: segments
                .into_iter()
                .filter(|(_, bytes)| !bytes.is_empty())
                .map(|(start, bytes)| ShadowSegment { start, bytes })
                .collect(),
        }
    }

    pub fn contains(&self, addr: usize) -> bool {
        self.segments
            .iter()
            .any(|segment| addr >= segment.start && addr - segment.start < segment.bytes.len())
    }

    pub fn read_text_u32(&self, addr: usize) -> Option<u32> {
        self.segments.iter().find_map(|segment| {
            let offset = addr.checked_sub(segment.start)?;
            let end = offset.checked_add(4)?;
            if end <= segment.bytes.len() {
                Some(u32::from_le_bytes(
                    segment.bytes[offset..end].try_into().unwrap(),
                ))
            } else {
                None
            }
        })
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}
