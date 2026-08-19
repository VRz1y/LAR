//! Virtual /proc/ Filesystem Engine.
//!
//! Emulates `/proc/cpuinfo`, `/proc/self/maps`, `/proc/self/auxv`, and `/proc/self/cmdline`
//! to present a pristine Android ARM64 environment to guest applications and packers.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::RwLock;

/// Represents a virtual procfs file entry with generated content.
#[derive(Debug, Clone)]
pub struct VirtualFile {
    pub path: String,
    pub content: Vec<u8>,
}

impl VirtualFile {
    pub fn new(path: impl Into<String>, content: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            content: content.into(),
        }
    }
}

/// Tracks virtual open file descriptors for virtual procfs files.
#[derive(Debug, Clone)]
pub struct VirtualFileHandle {
    pub file_path: String,
    pub cursor: usize,
}

/// ProcFS Virtualization Provider.
pub struct VirtualProcFs {
    files: RwLock<HashMap<String, Vec<u8>>>,
    open_handles: RwLock<HashMap<i32, VirtualFileHandle>>,
    next_fd: AtomicI32,
}

impl Default for VirtualProcFs {
    fn default() -> Self {
        Self::new()
    }
}

impl VirtualProcFs {
    pub fn new() -> Self {
        let instance = Self {
            files: RwLock::new(HashMap::new()),
            open_handles: RwLock::new(HashMap::new()),
            // Use high virtual FDs (starting at 0x70000000) to avoid collisions with OS file descriptors
            next_fd: AtomicI32::new(0x7000_0000),
        };

        instance.init_default_proc_files();
        instance
    }

    /// Initializes standard Android /proc files.
    pub fn init_default_proc_files(&self) {
        let cpuinfo = "\
processor\t: 0
BogoMIPS\t: 38.40
Features\t: fp asimd evtstrm aes pmull sha1 sha2 crc32 atomics fphp asimdhp cpuid asimdrdm jscvt fcma lrcpc dcpop sha3 sm3 sm4 asimddp sha512 sve asimdfhm dit uscat ilrcpc flagm sb paca pacg dcpodp flagm2 frint
CPU implementer\t: 0x51
CPU architecture: 8
CPU variant\t: 0xd
CPU part\t: 0x805
CPU revision\t: 14

Hardware\t: Qualcomm Technologies, Inc SM8650
Revision\t: 0000
Serial\t\t: 0000000000000000
";

        let version = "Linux version 6.1.75-android15-11-g1234567 (android-build@google.com) #1 SMP PREEMPT 2026 aarch64\n";
        let cmdline = "com.example.android.app\0";

        self.set_file("/proc/cpuinfo", cpuinfo.as_bytes());
        self.set_file("/proc/version", version.as_bytes());
        self.set_file("/proc/self/cmdline", cmdline.as_bytes());
        self.set_file("/proc/self/maps", b"");
    }

    /// Sets or updates content of a virtual proc file.
    pub fn set_file(&self, path: &str, content: &[u8]) {
        let mut files = self.files.write().unwrap();
        files.insert(path.to_string(), content.to_vec());
    }

    /// Checks if a given path is handled by virtual ProcFS.
    pub fn is_virtual_path(&self, path: &str) -> bool {
        let p = path.trim_end_matches('/');
        p.starts_with("/proc/") || p == "/proc" || p.starts_with("/sys/")
    }

    /// Attempts to open a virtual proc file. Returns virtual FD if handled.
    pub fn open(&self, path: &str) -> Option<i32> {
        let files = self.files.read().unwrap();
        if files.contains_key(path) {
            let fd = self.next_fd.fetch_add(1, Ordering::Relaxed);
            let mut handles = self.open_handles.write().unwrap();
            handles.insert(
                fd,
                VirtualFileHandle {
                    file_path: path.to_string(),
                    cursor: 0,
                },
            );
            Some(fd)
        } else {
            None
        }
    }

    /// Reads from an open virtual file descriptor.
    pub fn read(&self, fd: i32, dest: &mut [u8]) -> Option<usize> {
        let mut handles = self.open_handles.write().unwrap();
        let handle = handles.get_mut(&fd)?;
        let files = self.files.read().unwrap();
        let content = files.get(&handle.file_path)?;

        if handle.cursor >= content.len() {
            return Some(0); // EOF
        }

        let available = &content[handle.cursor..];
        let to_copy = std::cmp::min(dest.len(), available.len());
        dest[..to_copy].copy_from_slice(&available[..to_copy]);
        handle.cursor += to_copy;

        Some(to_copy)
    }

    /// Closes a virtual file descriptor.
    pub fn close(&self, fd: i32) -> bool {
        let mut handles = self.open_handles.write().unwrap();
        handles.remove(&fd).is_some()
    }

    /// Checks if an FD is a virtual file descriptor.
    pub fn is_virtual_fd(&self, fd: i32) -> bool {
        let handles = self.open_handles.read().unwrap();
        handles.contains_key(&fd)
    }

    /// Updates sanitized virtual `/proc/self/maps` based on loaded library bases and memory regions.
    pub fn update_virtual_maps(&self, mappings: &[(usize, usize, &str, &str)]) {
        let mut maps_text = String::new();
        for &(start, end, perms, name) in mappings {
            maps_text.push_str(&format!(
                "{:016x}-{:016x} {} 00000000 00:00 0   {}\n",
                start, end, perms, name
            ));
        }
        self.set_file("/proc/self/maps", maps_text.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_procfs_cpuinfo_and_read() {
        let procfs = VirtualProcFs::new();
        assert!(procfs.is_virtual_path("/proc/cpuinfo"));
        assert!(procfs.is_virtual_path("/proc/self/maps"));

        let fd = procfs.open("/proc/cpuinfo").expect("Failed to open /proc/cpuinfo");
        assert!(procfs.is_virtual_fd(fd));

        let mut buf = [0u8; 128];
        let bytes_read = procfs.read(fd, &mut buf).unwrap();
        assert!(bytes_read > 0);
        let read_str = std::str::from_utf8(&buf[..bytes_read]).unwrap();
        assert!(read_str.contains("Qualcomm") || read_str.contains("processor"));

        assert!(procfs.close(fd));
        assert!(!procfs.is_virtual_fd(fd));
    }

    #[test]
    fn test_procfs_custom_maps() {
        let procfs = VirtualProcFs::new();
        let mappings = vec![
            (0x0040_0000, 0x0041_0000, "r-xp", "/system/lib64/libc.so"),
            (0x0041_0000, 0x0042_0000, "r--p", "/system/lib64/libc.so"),
        ];
        procfs.update_virtual_maps(&mappings);

        let fd = procfs.open("/proc/self/maps").unwrap();
        let mut buf = [0u8; 256];
        let n = procfs.read(fd, &mut buf).unwrap();
        let s = std::str::from_utf8(&buf[..n]).unwrap();
        assert!(s.contains("0000000000400000-0000000000410000 r-xp"));
        assert!(s.contains("libc.so"));
    }
}
