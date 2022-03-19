//! Cup Channel Library
//!
//! *Yes cup-chan please swap my cups around uwu*
//!
//! Simple usage example:
//! ```rust
//! use cupchan::cupchan;
//!
//! let (mut writer, reader) = cupchan(0);
//! *writer = 1;
//! writer.flush();
//! assert_eq!(*reader, 1);
//!
//! *writer = 2;
//! writer.flush();
//! assert_eq!(*reader, 2);
//!
//! drop(reader);
//!
//! let reader = writer.new_reader().unwrap(); // Create a new reader
//!
//! *writer = 3;
//! writer.flush();
//! assert_eq!(*reader, 3);
//! ```

use std::{
    ops::{Deref, DerefMut},
    ptr,
};

#[cfg(loom)]
pub(crate) use loom::{
    cell::{ConstPtr, MutPtr, UnsafeCell},
    sync::atomic::{AtomicU8, Ordering},
};

#[cfg(not(loom))]
pub(crate) use std::{
    cell::UnsafeCell,
    sync::atomic::{AtomicU8, Ordering},
};

const OBJECT_PERMUTATIONS: &[u8; 6] = &[
    0b000, // <W><S><R>
    0b001, // <W><R><S>
    0b010, // <S><R><W>
    0b011, // <S><W><R>
    0b100, // <R><S><W>
    0b101, // <R><W><S>
];
const WRITER_PERMUTATIONS: &[usize; 6] = &[0, 0, 2, 1, 2, 1];
const READER_PERMUTATIONS: &[usize; 6] = &[2, 1, 1, 2, 0, 0];
const WRITER_SWAP_PERMUTATIONS: &[u8; 6] = &[
    0b011, // <S><W><R>
    0b010, // <S><R><W>
    0b001, // <W><R><S>
    0b000, // <W><S><R>
    0b101, // <R><W><S>
    0b100, // <R><S><W>
];
const READER_SWAP_PERMUTATIONS: &[u8; 6] = &[
    0b001, // <W><R><S>
    0b000, // <W><S><R>
    0b100, // <R><S><W>
    0b101, // <R><W><S>
    0b010, // <S><R><W>
    0b011, // <S><W><R>
];
const PERMUTATION_MASK: u8 = 0b00000111;

const WRITER_LOCK: u8 = 0b10000000; // XOR with state to toggle
const READER_LOCK: u8 = 0b01000000; // XOR with state to toggle
const UPDATE_FLAG: u8 = 0b00100000; // OR with state to set

/// A simple async channel used to quickly update data between threads
/// Useful in a situation where you need to model some read-only state on a receiving thread that can be periodically, but quickly, updated from a writer thread.
#[derive(Debug)]
struct Cupchan<T> {
    // One of these cups is reading, one writing, one for intermediate storage, which one is which depends on the permutation state
    cups: [UnsafeCell<T>; 3],
    // least significant bits = represents the permutation of cups (of which there are 6) i.e. which one is the reader, writer, and storage
    // most significant bits = (writer lock, reader lock, whether storage was just updated)
    state: AtomicU8,
}
/// Create a new Cup Channel
pub fn cupchan<T: Clone>(initial: T) -> (CupchanWriter<T>, CupchanReader<T>) {
    let chan = Cupchan {
        cups: [
            UnsafeCell::new(initial.clone()),
            UnsafeCell::new(initial.clone()),
            UnsafeCell::new(initial),
        ],
        state: AtomicU8::new(OBJECT_PERMUTATIONS[0] ^ WRITER_LOCK ^ READER_LOCK), // Initial state: read & write are locked, <W><S><R> permutation
    };
    let chan = Box::leak(Box::new(chan)); // Use special dropping logic based on state
    (CupchanWriter { chan }, CupchanReader { chan })
}

// when created, modify state to set writer lock flag
// when dropped, modify permutation to swap writer & storage object, unset writer lock flag, set storage new flag
/// Write to the Cup Channel, make sure to call flush() afterwards.
pub struct CupchanWriter<T: 'static> {
    chan: &'static Cupchan<T>,
}
impl<T> CupchanWriter<T> {
    pub fn flush(&mut self) {
        // Needs exclusive reference
        // Update storage flag & swap cups
        let _ = self
            .chan
            .state
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                let res = (state & !PERMUTATION_MASK)
                    ^ WRITER_SWAP_PERMUTATIONS[(state & PERMUTATION_MASK) as usize]
                    | UPDATE_FLAG;
                println!(
                    "[write] new permutation, reader: {}, writer: {}, update?: {}",
                    READER_PERMUTATIONS[(res & PERMUTATION_MASK) as usize],
                    WRITER_PERMUTATIONS[(res & PERMUTATION_MASK) as usize],
                    res & UPDATE_FLAG
                );
                Some(res)
            });
    }
    #[inline]
    fn write_index(&self) -> usize {
        let state = self.chan.state.load(Ordering::Acquire); // Make sure this is after all state changes
        WRITER_PERMUTATIONS[(state & PERMUTATION_MASK) as usize]
    }
    #[cfg(loom)]
    pub fn loom_ptr(&mut self) -> MutPtr<T> {
        let write_index = self.write_index();
        println!("[write] index: {:?}", write_index);
        self.chan.cups[write_index].get_mut()
    }
    pub fn print(&self)
    where
        T: std::fmt::Debug,
    {
        println!(
            "[write] state: {:?}, {:0>8b}",
            &self.chan.cups,
            self.chan.state.load(Ordering::SeqCst)
        );
    }
    pub fn new_reader(&self) -> Option<CupchanReader<T>> {
        // Toggle READER_LOCK bit if not set
        let res = self
            .chan
            .state
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |state| {
                if state & READER_LOCK == 0 {
                    Some(state ^ READER_LOCK)
                } else {
                    None
                }
            });
        res.ok().map(|_| CupchanReader { chan: self.chan })
    }
}
#[cfg(not(loom))]
impl<T> Deref for CupchanWriter<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &(*self.chan.cups[self.write_index()].get()) }
    }
}
#[cfg(not(loom))]
impl<T> DerefMut for CupchanWriter<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut (*self.chan.cups[self.write_index()].get()) }
    }
}
impl<T> Drop for CupchanWriter<T> {
    fn drop(&mut self) {
        let state = self.chan.state.fetch_xor(WRITER_LOCK, Ordering::AcqRel);
        if state & READER_LOCK == 0 {
            // Safety: self.chan was created using Box::leak() and the lock mask system prevents duplicate frees
            unsafe {
                let ptr = std::mem::transmute::<_, *mut Cupchan<T>>(self.chan);
                ptr::drop_in_place(ptr);
            }
        }
    }
}
unsafe impl<T> Send for CupchanWriter<T> where T: Send + Sync {}

// when created, modify state to set reader lock flag
// when dropped, modify state permutation to swap reader & storage object, unset reader lock flag, unset storage new flag
/// Read from the Cup Channel by dereferencing this obejct
pub struct CupchanReader<T: 'static> {
    chan: &'static Cupchan<T>,
}
impl<T> CupchanReader<T> {
    pub fn new_writer(&self) -> Option<CupchanWriter<T>> {
        // Toggle WRITER_LOCK bit if not set
        let res = self
            .chan
            .state
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |state| {
                if state & WRITER_LOCK == 0 {
                    Some(state ^ WRITER_LOCK)
                } else {
                    None
                }
            });
        res.ok().map(|_| CupchanWriter { chan: self.chan })
    }
    #[inline]
    fn read_index(&self) -> usize {
        let res = self
            .chan
            .state
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                println!(
                    "[read] current permutation, reader: {}, writer: {}, update?: {}",
                    READER_PERMUTATIONS[(state & PERMUTATION_MASK) as usize],
                    WRITER_PERMUTATIONS[(state & PERMUTATION_MASK) as usize],
                    state & UPDATE_FLAG
                );
                if state & UPDATE_FLAG != 0 {
                    let res = (state & !PERMUTATION_MASK)
                        ^ READER_SWAP_PERMUTATIONS[(state & PERMUTATION_MASK) as usize]
                        ^ UPDATE_FLAG;
                    println!(
                        "[read]  new permutation, reader: {}, writer: {}",
                        READER_PERMUTATIONS[(res & PERMUTATION_MASK) as usize],
                        WRITER_PERMUTATIONS[(res & PERMUTATION_MASK) as usize]
                    );
                    Some(res)
                } else {
                    None
                }
            });
        READER_PERMUTATIONS[match res {
            Ok(r) => READER_SWAP_PERMUTATIONS[(r & PERMUTATION_MASK) as usize] as usize,
            Err(r) => (r & PERMUTATION_MASK) as usize,
        }]
    }
    #[cfg(loom)]
    pub fn loom_ptr(&self) -> ConstPtr<T> {
        let read_index = self.read_index();
        println!("[read]  index: {:?}", read_index);
        unsafe { self.chan.cups[read_index].get() }
    }
    pub fn print(&self)
    where
        T: std::fmt::Debug,
    {
        println!(
            "read state: {:?}, {:0>8b}",
            &self.chan.cups,
            self.chan.state.load(Ordering::SeqCst)
        );
    }
}
#[cfg(not(loom))]
impl<T> Deref for CupchanReader<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &(*self.chan.cups[self.read_index()].get()) }
    }
}
impl<T> Drop for CupchanReader<T> {
    fn drop(&mut self) {
        let prev_state = self.chan.state.fetch_xor(READER_LOCK, Ordering::AcqRel);
        if prev_state & WRITER_LOCK == 0 {
            // Safety: self.chan was created using Box::leak() and the lock mask system prevents duplicate frees
            unsafe {
                let ptr = std::mem::transmute::<_, *mut Cupchan<T>>(self.chan);
                ptr::drop_in_place(ptr);
            }
        }
    }
}
unsafe impl<T> Send for CupchanReader<T> where T: Send + Sync {}

#[cfg(test)]
mod tests {
    use std::thread;

    use crate::cupchan;

    #[test]
    fn test_chan_sync() {
        let (mut writer, reader) = cupchan(0);
        *writer = 1;
        writer.flush();
        assert_eq!(*reader, 1);

        *writer = 2;
        writer.flush();
        assert_eq!(*reader, 2);

        drop(reader);

        let reader = writer.new_reader().unwrap();

        *writer = 3;
        writer.flush();
        assert_eq!(*reader, 3);
        drop(writer)
    }

    #[test]
    fn test_chan_async() {
        let (mut writer, reader) = cupchan(0);

        const MAX: usize = 20000;
        let join = thread::spawn(move || {
            for i in 0..MAX {
                *writer = i;
                writer.flush();
            }
        });

        let mut current = *reader;
        // let mut array = [0usize; MAX];
        while current < MAX - 1 {
            current = *reader;
        }
        // print!("{:?}", array);
        assert!(*reader == MAX - 1);

        join.join().unwrap();
    }
}
