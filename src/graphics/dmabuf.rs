use std::fmt;
use std::ops::{Deref, DerefMut};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::sync::Arc;

#[derive(Debug)]
pub struct DmaBufMapping {
    ptr: *mut libc::c_void,
    len: usize,
    view_start: usize,
    view_len: usize,
}

impl DmaBufMapping {
    pub fn as_slice(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts((self.ptr as *const u8).add(self.view_start), self.view_len)
        }
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe {
            std::slice::from_raw_parts_mut(
                (self.ptr as *mut u8).add(self.view_start),
                self.view_len,
            )
        }
    }
}
impl Deref for DmaBufMapping {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}
impl DerefMut for DmaBufMapping {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}
impl Drop for DmaBufMapping {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr, self.len);
        }
    }
}

#[derive(Debug, Clone)]
pub struct DmaBufPlane {
    fd: Arc<OwnedFd>,
    pub offset: u32,
    pub stride: u32,
    pub size: u64,
    pub modifier: u64,
}
impl DmaBufPlane {
    pub fn from_owned_fd(
        fd: OwnedFd,
        offset: u32,
        stride: u32,
        size: u64,
        modifier: u64,
    ) -> Result<Self, DmaBufError> {
        let plane = Self {
            fd: Arc::new(fd),
            offset,
            stride,
            size,
            modifier,
        };
        plane.validate()?;
        Ok(plane)
    }
    pub fn duplicate(
        fd: BorrowedFd<'_>,
        offset: u32,
        stride: u32,
        size: u64,
        modifier: u64,
    ) -> Result<Self, DmaBufError> {
        let duplicated = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
        if duplicated < 0 {
            return Err(DmaBufError::DuplicateFailed(errno()));
        }
        Self::from_owned_fd(
            unsafe { OwnedFd::from_raw_fd(duplicated) },
            offset,
            stride,
            size,
            modifier,
        )
    }
    pub fn validate(&self) -> Result<(), DmaBufError> {
        if self.stride == 0 || self.size < self.offset as u64 {
            return Err(DmaBufError::InvalidPlane);
        }
        Ok(())
    }
    pub fn borrowed_fd(&self) -> Result<BorrowedFd<'_>, DmaBufError> {
        self.validate()?;
        Ok(self.fd.as_fd())
    }
    pub fn fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
    pub fn mmap_writable(&self) -> Result<DmaBufMapping, DmaBufError> {
        self.validate()?;
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
        if page == 0 {
            return Err(DmaBufError::MapFailed(libc::EINVAL));
        }
        let aligned = (self.offset as u64 / page) * page;
        let view_len = usize::try_from(self.size - self.offset as u64)
            .map_err(|_| DmaBufError::MapFailed(libc::EOVERFLOW))?;
        let view_start = usize::try_from(self.offset as u64 - aligned)
            .map_err(|_| DmaBufError::MapFailed(libc::EOVERFLOW))?;
        let len = view_start
            .checked_add(view_len)
            .ok_or(DmaBufError::MapFailed(libc::EOVERFLOW))?;
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                self.fd(),
                aligned as libc::off_t,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(DmaBufError::MapFailed(errno()));
        }
        Ok(DmaBufMapping {
            ptr,
            len,
            view_start,
            view_len,
        })
    }
}
impl PartialEq for DmaBufPlane {
    fn eq(&self, other: &Self) -> bool {
        self.fd() == other.fd()
            && self.offset == other.offset
            && self.stride == other.stride
            && self.size == other.size
            && self.modifier == other.modifier
    }
}
impl Eq for DmaBufPlane {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FenceKind {
    Acquire,
    Release,
}
#[derive(Debug)]
pub struct SyncFence {
    fd: OwnedFd,
    kind: FenceKind,
}
impl SyncFence {
    pub fn from_owned_fd(fd: OwnedFd, kind: FenceKind) -> Self {
        Self { fd, kind }
    }
    pub fn duplicate(fd: BorrowedFd<'_>, kind: FenceKind) -> Result<Self, DmaBufError> {
        let duplicated = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
        if duplicated < 0 {
            return Err(DmaBufError::DuplicateFailed(errno()));
        }
        Ok(Self {
            fd: unsafe { OwnedFd::from_raw_fd(duplicated) },
            kind,
        })
    }
    pub fn fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
    pub fn kind(&self) -> FenceKind {
        self.kind
    }
    pub fn is_signaled(&self) -> Result<bool, DmaBufError> {
        let mut pollfd = libc::pollfd {
            fd: self.fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut pollfd, 1, 0) };
        if result < 0 {
            return Err(DmaBufError::PollFailed(errno()));
        }
        Ok(result > 0 && (pollfd.revents & (libc::POLLIN | libc::POLLHUP)) != 0)
    }
}
impl AsRawFd for SyncFence {
    fn as_raw_fd(&self) -> RawFd {
        self.fd()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DmaBufError {
    InvalidPlane,
    InvalidFence,
    PollFailed(i32),
    DuplicateFailed(i32),
    MapFailed(i32),
}
impl fmt::Display for DmaBufError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DMA-BUF error: {self:?}")
    }
}
impl std::error::Error for DmaBufError {}
fn errno() -> i32 {
    unsafe { *libc::__errno_location() }
}
