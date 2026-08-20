use std::fmt;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::sync::Arc;

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
        let owned = unsafe { OwnedFd::from_raw_fd(duplicated) };
        Self::from_owned_fd(owned, offset, stride, size, modifier)
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
}

impl fmt::Display for DmaBufError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPlane => write!(f, "invalid DMA-BUF plane"),
            Self::InvalidFence => write!(f, "invalid sync fence"),
            Self::PollFailed(e) => write!(f, "fence poll failed (errno {})", e),
            Self::DuplicateFailed(e) => write!(f, "DMA-BUF duplication failed (errno {})", e),
        }
    }
}
impl std::error::Error for DmaBufError {}

fn errno() -> i32 {
    unsafe { *libc::__errno_location() }
}
