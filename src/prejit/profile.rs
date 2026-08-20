//! Android Baseline Profile Parser (`assets/dexopt/baseline.prof`).
//!
//! Extracts hot startup classes and methods defined in ART baseline profile
//! to guide Install-Time Pre-JIT prioritization.

/// Android Baseline Profile Magic ("pro\0" or "prof").
pub const PROFILE_MAGIC_V015: [u8; 4] = [b'p', b'r', b'o', 0];
pub const PROFILE_MAGIC_V010: [u8; 4] = [b'p', b'r', b'o', b'm'];

/// Summary of hot startup methods and classes extracted from baseline profile.
#[derive(Debug, Clone, Default)]
pub struct BaselineProfileSummary {
    pub hot_method_count: usize,
    pub startup_classes: Vec<String>,
    pub dex_files_referenced: Vec<String>,
}

/// Baseline Profile Parser.
pub struct BaselineProfileParser;

impl BaselineProfileParser {
    /// Parses `baseline.prof` bytes and extracts startup profile metadata.
    pub fn parse(bytes: &[u8]) -> Result<BaselineProfileSummary, String> {
        if bytes.len() < 8 {
            return Err("Profile buffer too small".to_string());
        }

        let mut summary = BaselineProfileSummary::default();

        // Check magic
        let magic = &bytes[0..4];
        if magic != PROFILE_MAGIC_V015 && magic != PROFILE_MAGIC_V010 && magic != b"PPRO" {
            // Text-based or raw binary profile fallback
            let text = String::from_utf8_lossy(bytes);
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('L') && trimmed.ends_with(';') {
                    summary.startup_classes.push(trimmed.to_string());
                } else if trimmed.contains("->") {
                    summary.hot_method_count += 1;
                }
            }
            return Ok(summary);
        }

        let num_dex_files = u16::from_le_bytes(bytes[4..6].try_into().unwrap_or([0, 0])) as usize;
        let num_methods = u16::from_le_bytes(bytes[6..8].try_into().unwrap_or([0, 0])) as usize;

        summary.hot_method_count = num_methods;
        for i in 0..num_dex_files {
            summary.dex_files_referenced.push(format!(
                "classes{}.dex",
                if i == 0 {
                    "".to_string()
                } else {
                    (i + 1).to_string()
                }
            ));
        }

        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_text_profile() {
        let sample_prof = "\
Lcom/example/MainActivity;
Lcom/example/Renderer;->init()V
Lcom/example/Audio;->play()V
";
        let summary = BaselineProfileParser::parse(sample_prof.as_bytes()).unwrap();
        assert_eq!(summary.startup_classes.len(), 1);
        assert_eq!(summary.hot_method_count, 2);
    }
}
