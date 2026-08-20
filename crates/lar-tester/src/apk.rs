use crate::axml::decode_manifest;
use lar::api_policy::{AndroidApi, ApiPolicy, ApiPolicyError};
use lar::dex::{DexError, DexFileMetadata, DexMetadata, DexReader, MultidexMetadata};
use lar::managers::ApplicationInfo;
use std::fmt;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

const EOCD: u32 = 0x0605_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const LOCAL: u32 = 0x0403_4b50;
const MAX_ENTRY: usize = 256 * 1024 * 1024;

#[derive(Debug)]
pub enum ApkError {
    Io(std::io::Error),
    InvalidZipFormat,
    NoArm64LibrariesFound,
    CorruptedEntry(String),
    InvalidDex(DexError),
    UnsafePath(String),
    DuplicateEntry(String),
    Incompatible(ApiPolicyError),
}

impl fmt::Display for ApkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {}", e),
            Self::InvalidZipFormat => write!(f, "Invalid APK/ZIP archive format"),
            Self::NoArm64LibrariesFound => write!(f, "No 64-bit ARM libraries found in APK"),
            Self::CorruptedEntry(e) => write!(f, "Corrupted APK entry: {}", e),
            Self::InvalidDex(e) => write!(f, "Invalid classes.dex: {}", e),
            Self::UnsafePath(e) => write!(f, "Unsafe APK path: {}", e),
            Self::DuplicateEntry(e) => write!(f, "Duplicate APK entry: {}", e),
            Self::Incompatible(e) => write!(f, "Incompatible APK: {:?}", e),
        }
    }
}
impl std::error::Error for ApkError {}
impl From<std::io::Error> for ApkError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<DexError> for ApkError {
    fn from(e: DexError) -> Self {
        Self::InvalidDex(e)
    }
}

#[derive(Debug, Clone)]
pub struct ApkNativeLib {
    pub path_in_apk: String,
    pub name: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApkMetadata {
    pub package: Option<String>,
    pub launcher_activity: Option<String>,
    pub manifest_xml: Option<Vec<u8>>,
    pub dex: Option<DexMetadata>,
    pub min_sdk: Option<u32>,
    pub target_sdk: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledApk {
    pub application: ApplicationInfo,
    pub multidex: MultidexMetadata,
    pub native_library_paths: Vec<PathBuf>,
}

#[derive(Clone, Copy)]
struct Entry {
    method: u16,
    compressed: usize,
    uncompressed: usize,
    offset: usize,
}

pub struct ApkReader;

impl ApkReader {
    pub fn check_compatibility(metadata: &ApkMetadata, api: AndroidApi) -> Result<(), ApkError> {
        ApiPolicy::default()
            .check_apk_compatibility(api, metadata.min_sdk, metadata.target_sdk)
            .map_err(ApkError::Incompatible)
    }
    pub fn install<P: AsRef<Path>, Q: AsRef<Path>>(
        apk_path: P,
        install_root: Q,
    ) -> Result<InstalledApk, ApkError> {
        Self::install_from_memory_for_api(&fs::read(apk_path)?, install_root, AndroidApi::API_36)
    }

    pub fn install_from_memory_for_api<P: AsRef<Path>>(
        bytes: &[u8],
        install_root: P,
        api: AndroidApi,
    ) -> Result<InstalledApk, ApkError> {
        let metadata = Self::read_metadata_from_memory(bytes)?;
        Self::check_compatibility(&metadata, api)?;
        Self::install_compatible_from_memory(bytes, install_root, metadata)
    }

    pub fn install_from_memory<P: AsRef<Path>>(
        bytes: &[u8],
        install_root: P,
    ) -> Result<InstalledApk, ApkError> {
        let metadata = Self::read_metadata_from_memory(bytes)?;
        Self::check_compatibility(&metadata, AndroidApi::API_36)?;
        Self::install_compatible_from_memory(bytes, install_root, metadata)
    }

    fn install_compatible_from_memory<P: AsRef<Path>>(
        bytes: &[u8],
        install_root: P,
        metadata: ApkMetadata,
    ) -> Result<InstalledApk, ApkError> {
        let package = metadata
            .package
            .clone()
            .ok_or_else(|| ApkError::CorruptedEntry("manifest package is missing".into()))?;
        validate_component(&package)?;
        let package_root = install_root.as_ref().join(&package);
        fs::create_dir_all(&package_root)?;
        let entries = central_entries(bytes)?;
        let mut dex_files = Vec::new();
        let mut native_library_paths = Vec::new();
        for (entry, name) in entries {
            if is_dex_name(&name) {
                let data = read_entry(bytes, entry)?;
                let path = package_root.join("dex").join(&name);
                write_atomic(&path, &data)?;
                dex_files.push(DexFileMetadata {
                    name,
                    path,
                    metadata: DexReader::read(&data)?,
                });
            } else if is_arm64_library(&name) {
                let library_name = name.rsplit('/').next().unwrap_or_default();
                validate_component(library_name)?;
                let data = read_entry(bytes, entry)?;
                let path = package_root
                    .join("lib")
                    .join("arm64-v8a")
                    .join(library_name);
                write_atomic(&path, &data)?;
                native_library_paths.push(path);
            }
        }
        dex_files.sort_by_key(|a| dex_sort_key(&a.name));
        if dex_files.is_empty() {
            return Err(ApkError::CorruptedEntry("classes.dex is missing".into()));
        }
        let dex_path = dex_files.first().map(|file| file.path.clone());
        let dex = dex_files.first().map(|file| file.metadata.clone());
        let native_libraries = native_library_paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        Ok(InstalledApk {
            application: ApplicationInfo {
                package,
                launcher_activity: metadata.launcher_activity,
                dex_path,
                dex,
                native_libraries,
            },
            multidex: MultidexMetadata { files: dex_files },
            native_library_paths,
        })
    }

    pub fn extract_arm64_libs<P: AsRef<Path>>(p: P) -> Result<Vec<ApkNativeLib>, ApkError> {
        Self::extract_arm64_libs_from_memory(&fs::read(p)?)
    }

    pub fn read_metadata<P: AsRef<Path>>(p: P) -> Result<ApkMetadata, ApkError> {
        Self::read_metadata_from_memory(&fs::read(p)?)
    }

    pub fn read_metadata_from_memory(bytes: &[u8]) -> Result<ApkMetadata, ApkError> {
        let entries = central_entries(bytes)?;
        let (entry, _) = entries
            .into_iter()
            .find(|(_, n)| n == "AndroidManifest.xml")
            .ok_or_else(|| ApkError::CorruptedEntry("AndroidManifest.xml is missing".into()))?;
        let manifest = read_entry(bytes, entry)?;
        parse_manifest(&manifest)
            .map(|mut m| {
                m.manifest_xml = Some(manifest);
                m
            })
            .map_err(|e| ApkError::CorruptedEntry(e.into()))
    }

    pub fn read_application_from_memory(bytes: &[u8]) -> Result<ApplicationInfo, ApkError> {
        let metadata = Self::read_metadata_from_memory(bytes)?;
        let dex = central_entries(bytes)?
            .into_iter()
            .find(|(_, name)| name == "classes.dex")
            .map(|(entry, _)| {
                read_entry(bytes, entry)
                    .and_then(|data| DexReader::read(&data).map_err(ApkError::from))
            })
            .transpose()?;
        let native_libraries = central_entries(bytes)?
            .into_iter()
            .filter(|(_, name)| is_arm64_library(name))
            .map(|(_, name)| name)
            .collect();
        Ok(ApplicationInfo {
            package: metadata
                .package
                .ok_or_else(|| ApkError::CorruptedEntry("manifest package is missing".into()))?,
            launcher_activity: metadata.launcher_activity,
            dex_path: None,
            dex,
            native_libraries,
        })
    }

    pub fn extract_arm64_libs_from_memory(bytes: &[u8]) -> Result<Vec<ApkNativeLib>, ApkError> {
        let entries = central_entries(bytes)?;
        let mut libs = Vec::new();
        for (entry, filename) in entries {
            if (filename.starts_with("lib/arm64-v8a/") || filename.starts_with("lib/aarch64/"))
                && filename.ends_with(".so")
            {
                let data = read_entry(bytes, entry)?;
                let name = filename.rsplit('/').next().unwrap_or(&filename).to_string();
                libs.push(ApkNativeLib {
                    path_in_apk: filename,
                    name,
                    data,
                });
            }
        }
        if libs.is_empty() {
            Err(ApkError::NoArm64LibrariesFound)
        } else {
            Ok(libs)
        }
    }
}

fn is_arm64_library(name: &str) -> bool {
    (name.starts_with("lib/arm64-v8a/") || name.starts_with("lib/aarch64/"))
        && name.ends_with(".so")
        && name.matches('/').count() == 2
}

fn is_dex_name(name: &str) -> bool {
    if name == "classes.dex" {
        return true;
    }
    let Some(number) = name
        .strip_prefix("classes")
        .and_then(|s| s.strip_suffix(".dex"))
    else {
        return false;
    };
    !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) && number != "1"
}

fn dex_sort_key(name: &str) -> usize {
    if name == "classes.dex" {
        1
    } else {
        name[7..name.len() - 4].parse().unwrap_or(usize::MAX)
    }
}

fn validate_component(value: &str) -> Result<(), ApkError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
    {
        return Err(ApkError::UnsafePath(value.to_owned()));
    }
    Ok(())
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<(), ApkError> {
    let parent = path
        .parent()
        .ok_or_else(|| ApkError::UnsafePath(path.display().to_string()))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name().unwrap().to_string_lossy(),
        std::process::id()
    ));
    let result = (|| {
        let mut file = File::create(&temp)?;
        file.write_all(data)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        Ok::<(), std::io::Error>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(ApkError::Io)
}

fn u16at(b: &[u8], p: usize) -> Result<usize, ApkError> {
    b.get(p..p + 2)
        .map(|x| u16::from_le_bytes([x[0], x[1]]) as usize)
        .ok_or(ApkError::InvalidZipFormat)
}
fn u32at(b: &[u8], p: usize) -> Result<usize, ApkError> {
    b.get(p..p + 4)
        .map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]) as usize)
        .ok_or(ApkError::InvalidZipFormat)
}
fn central_entries(b: &[u8]) -> Result<Vec<(Entry, String)>, ApkError> {
    let start = b.len().saturating_sub(22 + 65_535);
    let pos = (start..b.len().saturating_sub(3))
        .rev()
        .find(|&p| b.get(p..p + 4) == Some(&EOCD.to_le_bytes()))
        .ok_or(ApkError::InvalidZipFormat)?;
    let count = u16at(b, pos + 10)?;
    let cd_size = u32at(b, pos + 12)?;
    let cd_off = u32at(b, pos + 16)?;
    if count == 0xffff
        || cd_size > b.len()
        || cd_off
            .checked_add(cd_size)
            .filter(|&x| x <= b.len())
            .is_none()
    {
        return Err(ApkError::InvalidZipFormat);
    }
    let mut p = cd_off;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if p.checked_add(46).filter(|&x| x <= b.len()).is_none() || u32at(b, p)? as u32 != CENTRAL {
            return Err(ApkError::InvalidZipFormat);
        }
        let method = u16at(b, p + 10)? as u16;
        let compressed = u32at(b, p + 20)?;
        let uncompressed = u32at(b, p + 24)?;
        let nl = u16at(b, p + 28)?;
        let xl = u16at(b, p + 30)?;
        let cl = u16at(b, p + 32)?;
        let off = u32at(b, p + 42)?;
        let ns = p + 46;
        let ne = ns.checked_add(nl).ok_or(ApkError::InvalidZipFormat)?;
        if ne > b.len()
            || p.checked_add(46 + nl + xl + cl)
                .filter(|&x| x <= b.len())
                .is_none()
        {
            return Err(ApkError::InvalidZipFormat);
        }
        let name = String::from_utf8_lossy(&b[ns..ne]).into_owned();
        out.push((
            Entry {
                method,
                compressed,
                uncompressed,
                offset: off,
            },
            name,
        ));
        p += 46 + nl + xl + cl;
    }
    Ok(out)
}
fn read_entry(b: &[u8], e: Entry) -> Result<Vec<u8>, ApkError> {
    if e.uncompressed > MAX_ENTRY || e.compressed > b.len() {
        return Err(ApkError::CorruptedEntry("entry exceeds size limit".into()));
    }
    if e.offset.checked_add(30).filter(|&x| x <= b.len()).is_none()
        || u32at(b, e.offset)? as u32 != LOCAL
    {
        return Err(ApkError::InvalidZipFormat);
    }
    let nl = u16at(b, e.offset + 26)?;
    let xl = u16at(b, e.offset + 28)?;
    let s = e
        .offset
        .checked_add(30 + nl + xl)
        .ok_or(ApkError::InvalidZipFormat)?;
    let end = s
        .checked_add(e.compressed)
        .filter(|&x| x <= b.len())
        .ok_or(ApkError::InvalidZipFormat)?;
    match e.method {
        0 => {
            if e.compressed != e.uncompressed {
                return Err(ApkError::CorruptedEntry("stored size mismatch".into()));
            }
            Ok(b[s..end].to_vec())
        }
        8 => inflate(&b[s..end], e.uncompressed),
        _ => Err(ApkError::CorruptedEntry(format!(
            "unsupported compression method {}",
            e.method
        ))),
    }
}

struct Bits<'a> {
    b: &'a [u8],
    p: usize,
    n: u32,
    v: u32,
}
impl<'a> Bits<'a> {
    fn get(&mut self, n: u32) -> Result<u32, ApkError> {
        while self.n < n {
            if self.p == self.b.len() {
                return Err(ApkError::InvalidZipFormat);
            }
            self.v |= (self.b[self.p] as u32) << self.n;
            self.p += 1;
            self.n += 8;
        }
        let x = self.v & ((1 << n) - 1);
        self.v >>= n;
        self.n -= n;
        Ok(x)
    }
    fn align(&mut self) {
        self.v = 0;
        self.n = 0;
    }
}
#[derive(Clone)]
struct Huff {
    codes: Vec<(u32, u8, u16)>,
}
fn huff(lengths: &[u8]) -> Result<Huff, ApkError> {
    let mut count = [0u16; 16];
    for &l in lengths {
        if l > 15 {
            return Err(ApkError::InvalidZipFormat);
        }
        if l != 0 {
            count[l as usize] += 1;
        }
    }
    let mut next = [0u32; 16];
    let mut code = 0;
    for i in 1..16 {
        code = (code + count[i - 1] as u32) << 1;
        next[i] = code;
    }
    let mut codes = Vec::new();
    for (symbol, &len) in lengths.iter().enumerate() {
        if len != 0 {
            let c = next[len as usize];
            next[len as usize] += 1;
            codes.push((rev(c, len), len, symbol as u16));
        }
    }
    Ok(Huff { codes })
}
fn rev(mut x: u32, n: u8) -> u32 {
    let mut y = 0;
    for _ in 0..n {
        y = (y << 1) | (x & 1);
        x >>= 1;
    }
    y
}
fn sym(bits: &mut Bits<'_>, h: &Huff) -> Result<u16, ApkError> {
    let mut x = 0;
    for n in 1..=15 {
        x |= bits.get(1)? << (n - 1);
        if let Some(&(_, _, s)) = h.codes.iter().find(|&&(c, l, _)| l == n && c == x) {
            return Ok(s);
        }
    }
    Err(ApkError::InvalidZipFormat)
}
fn inflate(input: &[u8], expected: usize) -> Result<Vec<u8>, ApkError> {
    let mut bits = Bits {
        b: input,
        p: 0,
        n: 0,
        v: 0,
    };
    let mut out = Vec::with_capacity(expected);
    let fixed_l = {
        let mut x = vec![0; 288];
        x[..=143].fill(8);
        x[144..=255].fill(9);
        x[256..=279].fill(7);
        x[280..288].fill(8);
        x
    };
    let fixed = (huff(&fixed_l)?, huff(&[5; 32])?);
    let extras = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    let bases = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    let dbases = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    let dextras = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
        13, 13,
    ];
    loop {
        let last = bits.get(1)? != 0;
        let kind = bits.get(2)?;
        let (lh, dh) = match kind {
            0 => {
                bits.align();
                let n = bits.get(16)? as usize;
                let inverse = bits.get(16)? as u16;
                if (n as u16) ^ inverse != u16::MAX {
                    return Err(ApkError::InvalidZipFormat);
                }
                let start = bits.p;
                let end = start.checked_add(n).ok_or(ApkError::InvalidZipFormat)?;
                if end > input.len() {
                    return Err(ApkError::InvalidZipFormat);
                };
                out.extend_from_slice(&input[start..end]);
                bits.p = end;
                (None, None)
            }
            1 => (Some(fixed.0.clone()), Some(fixed.1.clone())),
            2 => {
                let nl = bits.get(5)? as usize + 257;
                let nd = bits.get(5)? as usize + 1;
                let nc = bits.get(4)? as usize + 4;
                let order = [
                    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
                ];
                let mut cl = vec![0; 19];
                for i in 0..nc {
                    cl[order[i]] = bits.get(3)? as u8
                }
                let ch = huff(&cl)?;
                let mut all = Vec::with_capacity(nl + nd);
                while all.len() < nl + nd {
                    match sym(&mut bits, &ch)? {
                        s @ 0..=15 => all.push(s as u8),
                        16 => {
                            let v = *all.last().ok_or(ApkError::InvalidZipFormat)?;
                            for _ in 0..bits.get(2)? + 3 {
                                all.push(v)
                            }
                        }
                        17 => {
                            for _ in 0..bits.get(3)? + 3 {
                                all.push(0)
                            }
                        }
                        18 => {
                            for _ in 0..bits.get(7)? + 11 {
                                all.push(0)
                            }
                        }
                        _ => return Err(ApkError::InvalidZipFormat),
                    }
                }
                if all.len() != nl + nd {
                    return Err(ApkError::InvalidZipFormat);
                }
                (Some(huff(&all[..nl])?), Some(huff(&all[nl..])?))
            }
            _ => return Err(ApkError::InvalidZipFormat),
        };
        if let (Some(lh), Some(dh)) = (lh, dh) {
            loop {
                let s = sym(&mut bits, &lh)? as usize;
                if s < 256 {
                    out.push(s as u8)
                } else if s == 256 {
                    break;
                } else if s <= 285 {
                    let i = s - 257;
                    let mut len = bases[i];
                    len += if extras[i] > 0 {
                        bits.get(extras[i])? as usize
                    } else {
                        0
                    };
                    let ds = sym(&mut bits, &dh)? as usize;
                    if ds >= 30 {
                        return Err(ApkError::InvalidZipFormat);
                    };
                    let mut dist = dbases[ds];
                    dist += if dextras[ds] > 0 {
                        bits.get(dextras[ds])? as usize
                    } else {
                        0
                    };
                    if dist > out.len() {
                        return Err(ApkError::InvalidZipFormat);
                    };
                    for _ in 0..len {
                        let v = out[out.len() - dist];
                        out.push(v)
                    }
                } else {
                    return Err(ApkError::InvalidZipFormat);
                }
                if out.len() > expected {
                    return Err(ApkError::CorruptedEntry(
                        "deflate output exceeds declared size".into(),
                    ));
                }
            }
        }
        if last {
            break;
        }
    }
    if out.len() != expected {
        return Err(ApkError::CorruptedEntry(
            "deflate output size mismatch".into(),
        ));
    }
    Ok(out)
}

fn parse_manifest(data: &[u8]) -> Result<ApkMetadata, &'static str> {
    if data.starts_with(b"<") {
        let text = String::from_utf8_lossy(data);
        return Ok(ApkMetadata {
            package: attr(&text, "manifest", "package"),
            launcher_activity: launcher_text(&text),
            manifest_xml: None,
            dex: None,
            min_sdk: attr(&text, "uses-sdk", "android:minSdkVersion").and_then(|v| v.parse().ok()),
            target_sdk: attr(&text, "uses-sdk", "android:targetSdkVersion")
                .and_then(|v| v.parse().ok()),
        });
    }
    let decoded = decode_manifest(data)?;
    Ok(ApkMetadata {
        package: decoded.package,
        launcher_activity: decoded.launcher_activity,
        manifest_xml: None,
        dex: None,
        min_sdk: decoded.min_sdk,
        target_sdk: decoded.target_sdk,
    })
}
fn attr(text: &str, tag: &str, key: &str) -> Option<String> {
    let p = text.find(&format!("<{}", tag))?;
    let e = text[p..].find('>')? + p;
    let x = &text[p..e];
    let k = format!("{}=\"", key);
    let s = x.find(&k)? + k.len();
    Some(x[s..].split('"').next()?.to_string())
}
fn launcher_text(text: &str) -> Option<String> {
    text.find("android.intent.action.MAIN")
        .and_then(|p| text[..p].rfind("<activity"))
        .and_then(|p| attr(&text[p..], "activity", "android:name"))
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_deflate_is_decoded() {
        assert_eq!(
            inflate(&[0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00], 5).unwrap(),
            b"hello"
        );
    }

    #[test]
    fn deflate_output_limit_is_enforced() {
        assert!(inflate(&[0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00], 4).is_err());
    }
}
