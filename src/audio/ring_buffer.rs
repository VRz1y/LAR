use std::cell::UnsafeCell;
use std::collections::VecDeque;
use std::mem::MaybeUninit;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct SpscRingBuffer<T> {
    slots: Box<[UnsafeCell<MaybeUninit<T>>]>,
    capacity: u64,
    head: AtomicU64,
    tail: AtomicU64,
}

unsafe impl<T: Send> Send for SpscRingBuffer<T> {}
unsafe impl<T: Send> Sync for SpscRingBuffer<T> {}

impl<T> SpscRingBuffer<T> {
    pub fn new(capacity: usize) -> Result<Self, RingBufferError> {
        if capacity == 0 {
            return Err(RingBufferError::InvalidCapacity);
        }
        let slots = (0..capacity)
            .map(|_| UnsafeCell::new(MaybeUninit::uninit()))
            .collect();
        Ok(Self {
            slots,
            capacity: capacity as u64,
            head: AtomicU64::new(0),
            tail: AtomicU64::new(0),
        })
    }

    pub fn capacity(&self) -> usize {
        self.capacity as usize
    }

    pub fn len(&self) -> usize {
        (self.head.load(Ordering::Acquire) - self.tail.load(Ordering::Acquire)) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire) == self.tail.load(Ordering::Acquire)
    }

    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    pub fn push(&self, value: T) -> Result<(), T> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head - tail == self.capacity {
            return Err(value);
        }
        let slot = &self.slots[(head % self.capacity) as usize];
        unsafe { (*slot.get()).write(value) };
        self.head.store(head + 1, Ordering::Release);
        Ok(())
    }

    pub fn pop(&self) -> Option<T> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if tail == head {
            return None;
        }
        let slot = &self.slots[(tail % self.capacity) as usize];
        let value = unsafe { (*slot.get()).assume_init_read() };
        self.tail.store(tail + 1, Ordering::Release);
        Some(value)
    }
}

impl<T> Drop for SpscRingBuffer<T> {
    fn drop(&mut self) {
        while self.pop().is_some() {}
    }
}

pub struct MutexRingBuffer<T> {
    queue: Mutex<VecDeque<T>>,
    capacity: usize,
}

impl<T> MutexRingBuffer<T> {
    pub fn new(capacity: usize) -> Result<Self, RingBufferError> {
        if capacity == 0 {
            return Err(RingBufferError::InvalidCapacity);
        }
        Ok(Self {
            queue: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
        })
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.queue.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.lock().unwrap().is_empty()
    }

    pub fn push(&self, value: T) -> Result<(), T> {
        let mut queue = self.queue.lock().unwrap();
        if queue.len() == self.capacity {
            return Err(value);
        }
        queue.push_back(value);
        Ok(())
    }

    pub fn pop(&self) -> Option<T> {
        self.queue.lock().unwrap().pop_front()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingBufferError {
    InvalidCapacity,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spsc_preserves_fifo_order_and_capacity() {
        let buffer = SpscRingBuffer::new(2).unwrap();
        assert_eq!(buffer.push(1), Ok(()));
        assert_eq!(buffer.push(2), Ok(()));
        assert_eq!(buffer.push(3), Err(3));
        assert_eq!(buffer.pop(), Some(1));
        assert_eq!(buffer.push(3), Ok(()));
        assert_eq!(buffer.pop(), Some(2));
        assert_eq!(buffer.pop(), Some(3));
        assert_eq!(buffer.pop(), None);
    }

    #[test]
    fn mutex_buffer_preserves_fifo_order_and_capacity() {
        let buffer = MutexRingBuffer::new(2).unwrap();
        assert_eq!(buffer.push(1), Ok(()));
        assert_eq!(buffer.push(2), Ok(()));
        assert_eq!(buffer.push(3), Err(3));
        assert_eq!(buffer.pop(), Some(1));
        assert_eq!(buffer.pop(), Some(2));
        assert_eq!(buffer.pop(), None);
    }
}
