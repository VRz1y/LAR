use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexMetadata {
    pub version: String,
    pub checksum: u32,
    pub file_size: u32,
    pub string_count: u32,
    pub class_count: u32,
    pub strings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexFileMetadata {
    pub name: String,
    pub path: std::path::PathBuf,
    pub metadata: DexMetadata,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MultidexMetadata {
    pub files: Vec<DexFileMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DexError {
    TooSmall,
    InvalidMagic,
    InvalidHeader,
    OutOfBounds,
    InvalidString,
}

impl fmt::Display for DexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DEX error: {:?}", self)
    }
}
impl std::error::Error for DexError {}

pub struct DexReader;

impl DexReader {
    pub fn read(bytes: &[u8]) -> Result<DexMetadata, DexError> {
        if bytes.len() < 112 {
            return Err(DexError::TooSmall);
        }
        if &bytes[..4] != b"dex\n" || bytes[7] != 0 {
            return Err(DexError::InvalidMagic);
        }
        let version = std::str::from_utf8(&bytes[4..7])
            .map_err(|_| DexError::InvalidMagic)?
            .to_owned();
        let endian = u32at(bytes, 40)?;
        if endian != 0x1234_5678 {
            return Err(DexError::InvalidHeader);
        }
        let file_size = u32at(bytes, 32)?;
        let header_size = u32at(bytes, 36)?;
        if header_size != 112 || file_size as usize != bytes.len() {
            return Err(DexError::InvalidHeader);
        }
        let string_count = u32at(bytes, 56)?;
        let string_offset = u32at(bytes, 60)?;
        let class_count = u32at(bytes, 96)?;
        checked_table(bytes, string_offset, string_count, 4)?;
        checked_table(bytes, u32at(bytes, 100)?, u32at(bytes, 96)?, 4)?;
        if string_count > 16_384 {
            return Err(DexError::InvalidHeader);
        }
        let mut strings = Vec::with_capacity(string_count as usize);
        for i in 0..string_count as usize {
            let offset = u32at(bytes, string_offset as usize + i * 4)? as usize;
            strings.push(read_string(bytes, offset)?);
        }
        Ok(DexMetadata {
            version,
            checksum: u32at(bytes, 8)?,
            file_size,
            string_count,
            class_count,
            strings,
        })
    }
}

fn u32at(bytes: &[u8], offset: usize) -> Result<u32, DexError> {
    let value = bytes
        .get(offset..offset.checked_add(4).ok_or(DexError::OutOfBounds)?)
        .ok_or(DexError::OutOfBounds)?;
    Ok(u32::from_le_bytes(value.try_into().unwrap()))
}

fn checked_table(bytes: &[u8], offset: u32, count: u32, width: usize) -> Result<(), DexError> {
    let size = (count as usize)
        .checked_mul(width)
        .ok_or(DexError::OutOfBounds)?;
    if (offset as usize)
        .checked_add(size)
        .filter(|end| *end <= bytes.len())
        .is_none()
    {
        return Err(DexError::OutOfBounds);
    }
    Ok(())
}

fn read_string(bytes: &[u8], offset: usize) -> Result<String, DexError> {
    let mut cursor = offset;
    let mut length = 0usize;
    let mut shift = 0usize;
    loop {
        let byte = *bytes.get(cursor).ok_or(DexError::OutOfBounds)?;
        cursor += 1;
        length |= ((byte & 0x7f) as usize)
            .checked_shl(shift as u32)
            .ok_or(DexError::InvalidString)?;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 28 {
            return Err(DexError::InvalidString);
        }
        if length > 1_048_576 {
            return Err(DexError::InvalidString);
        }
    }
    let end = cursor.checked_add(length).ok_or(DexError::OutOfBounds)?;
    let data = bytes.get(cursor..end).ok_or(DexError::OutOfBounds)?;
    let nul = data
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(data.len());
    String::from_utf8(data[..nul].to_vec()).map_err(|_| DexError::InvalidString)
}
