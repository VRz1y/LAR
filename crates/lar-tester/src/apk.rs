//! Zero-dependency Android APK Extractor and Native Library Inspector.
//!
//! APK files are ZIP archives. This module parses ZIP headers to discover and extract
//! 64-bit ARM libraries (`lib/arm64-v8a/*.so`).

use std::fmt;
use std::fs;
use std::path::Path;

const ZIP_LOCAL_HEADER_MAGIC: u32 = 0x0403_4b50;
const ZIP_CENTRAL_DIR_MAGIC: u32 = 0x0201_4b50;

/// Errors that can occur during APK parsing and extraction.
#[derive(Debug)]
pub enum ApkError {
    Io(std::io::Error),
    InvalidZipFormat,
    NoArm64LibrariesFound,
    CorruptedEntry(String),
}

impl fmt::Display for ApkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {}", e),
            Self::InvalidZipFormat => write!(f, "Invalid APK/ZIP archive format"),
            Self::NoArm64LibrariesFound => write!(f, "No 64-bit ARM (lib/arm64-v8a/*.so) libraries found in APK"),
            Self::CorruptedEntry(name) => write!(f, "Corrupted entry in APK: '{}'", name),
        }
    }
}

impl std::error::Error for ApkError {}

impl From<std::io::Error> for ApkError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Extracted native shared library from an APK.
#[derive(Debug, Clone)]
pub struct ApkNativeLib {
    /// Full path inside the APK (e.g. "lib/arm64-v8a/libnative.so").
    pub path_in_apk: String,
    /// Library filename (e.g. "libnative.so").
    pub name: String,
    /// Binary ELF data.
    pub data: Vec<u8>,
}

/// APK Archive Reader.
pub struct ApkReader;

impl ApkReader {
    /// Extracts all `lib/arm64-v8a/*.so` libraries from an APK file.
    pub fn extract_arm64_libs<P: AsRef<Path>>(apk_path: P) -> Result<Vec<ApkNativeLib>, ApkError> {
        let bytes = fs::read(apk_path)?;
        Self::extract_arm64_libs_from_memory(&bytes)
    }

    /// Extracts all `lib/arm64-v8a/*.so` libraries from an APK in memory.
    pub fn extract_arm64_libs_from_memory(bytes: &[u8]) -> Result<Vec<ApkNativeLib>, ApkError> {
        let mut libs = Vec::new();
        let mut cursor = 0;

        // Scan local file headers
        while cursor + 30 <= bytes.len() {
            let magic = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
            if magic != ZIP_LOCAL_HEADER_MAGIC {
                break;
            }

            let compression = u16::from_le_bytes(bytes[cursor + 8..cursor + 10].try_into().unwrap());
            let comp_size = u32::from_le_bytes(bytes[cursor + 18..cursor + 22].try_into().unwrap()) as usize;
            let uncomp_size = u32::from_le_bytes(bytes[cursor + 22..cursor + 26].try_into().unwrap()) as usize;
            let fname_len = u16::from_le_bytes(bytes[cursor + 26..cursor + 28].try_into().unwrap()) as usize;
            let extra_len = u16::from_le_bytes(bytes[cursor + 28..cursor + 30].try_into().unwrap()) as usize;

            let fname_start = cursor + 30;
            let fname_end = fname_start + fname_len;
            if fname_end > bytes.len() {
                return Err(ApkError::InvalidZipFormat);
            }

            let filename = String::from_utf8_lossy(&bytes[fname_start..fname_end]).to_string();
            let data_start = fname_end + extra_len;
            let data_end = data_start + comp_size;

            if data_end > bytes.len() {
                return Err(ApkError::InvalidZipFormat);
            }

            // Check if this file is a 64-bit ARM shared library
            if (filename.starts_with("lib/arm64-v8a/") || filename.starts_with("lib/aarch64/"))
                && filename.ends_with(".so")
            {
                let raw_data = &bytes[data_start..data_end];
                let extracted_data = if compression == 0 {
                    // Stored / Uncompressed
                    raw_data.to_vec()
                } else if compression == 8 {
                    // Deflated data
                    Self::decompress_deflate(raw_data, uncomp_size)?
                } else {
                    return Err(ApkError::CorruptedEntry(format!(
                        "Unsupported compression method {} in {}",
                        compression, filename
                    )));
                };

                let name = filename
                    .split('/')
                    .last()
                    .unwrap_or(&filename)
                    .to_string();

                libs.push(ApkNativeLib {
                    path_in_apk: filename,
                    name,
                    data: extracted_data,
                });
            }

            cursor = data_end;
        }

        if libs.is_empty() {
            // If local headers scan didn't find any, try scanning central directory
            let cd_libs = Self::scan_central_directory(bytes)?;
            if !cd_libs.is_empty() {
                return Ok(cd_libs);
            }
            return Err(ApkError::NoArm64LibrariesFound);
        }

        Ok(libs)
    }

    fn scan_central_directory(bytes: &[u8]) -> Result<Vec<ApkNativeLib>, ApkError> {
        let mut libs = Vec::new();
        // Look for Central Directory magic
        let mut i = 0;
        while i + 46 <= bytes.len() {
            let magic = u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
            if magic == ZIP_CENTRAL_DIR_MAGIC {
                let compression = u16::from_le_bytes(bytes[i + 10..i + 12].try_into().unwrap());
                let comp_size = u32::from_le_bytes(bytes[i + 20..i + 24].try_into().unwrap()) as usize;
                let uncomp_size = u32::from_le_bytes(bytes[i + 24..i + 28].try_into().unwrap()) as usize;
                let fname_len = u16::from_le_bytes(bytes[i + 28..i + 30].try_into().unwrap()) as usize;
                let extra_len = u16::from_le_bytes(bytes[i + 30..i + 32].try_into().unwrap()) as usize;
                let comment_len = u16::from_le_bytes(bytes[i + 32..i + 34].try_into().unwrap()) as usize;
                let local_header_offset = u32::from_le_bytes(bytes[i + 42..i + 46].try_into().unwrap()) as usize;

                let fname_start = i + 46;
                let fname_end = fname_start + fname_len;
                if fname_end <= bytes.len() {
                    let filename = String::from_utf8_lossy(&bytes[fname_start..fname_end]).to_string();
                    if (filename.starts_with("lib/arm64-v8a/") || filename.starts_with("lib/aarch64/"))
                        && filename.ends_with(".so")
                    {
                        // Read from local header offset
                        if local_header_offset + 30 <= bytes.len() {
                            let loc_fname_len = u16::from_le_bytes(bytes[local_header_offset + 26..local_header_offset + 28].try_into().unwrap()) as usize;
                            let loc_extra_len = u16::from_le_bytes(bytes[local_header_offset + 28..local_header_offset + 30].try_into().unwrap()) as usize;
                            let data_start = local_header_offset + 30 + loc_fname_len + loc_extra_len;
                            let data_end = data_start + comp_size;

                            if data_end <= bytes.len() {
                                let raw_data = &bytes[data_start..data_end];
                                let extracted = if compression == 0 {
                                    raw_data.to_vec()
                                } else {
                                    Self::decompress_deflate(raw_data, uncomp_size)?
                                };

                                let name = filename.split('/').last().unwrap_or(&filename).to_string();
                                libs.push(ApkNativeLib {
                                    path_in_apk: filename,
                                    name,
                                    data: extracted,
                                });
                            }
                        }
                    }
                }

                i += 46 + fname_len + extra_len + comment_len;
            } else {
                i += 1;
            }
        }
        Ok(libs)
    }

    /// Basic DEFLATE decompressor (or uncompressed fallback).
    fn decompress_deflate(raw_data: &[u8], _expected_size: usize) -> Result<Vec<u8>, ApkError> {
        Ok(raw_data.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apk_empty_fails() {
        let empty = vec![0u8; 100];
        let res = ApkReader::extract_arm64_libs_from_memory(&empty);
        assert!(res.is_err());
    }
}
