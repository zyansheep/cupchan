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
#![feature(test)]

use std::{fmt, ptr};

#[cfg(loom)]
pub(crate) use loom::{
	cell::{ConstPtr, MutPtr, UnsafeCell},
	sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[cfg(not(loom))]
pub(crate) use std::{
	cell::UnsafeCell,
	ops::{Deref, DerefMut},
	sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

const OBJECT_PERMUTATIONS: &[usize; 6] = &[
	0b000, // <W><S><R>
	0b001, // <W><R><S>
	0b010, // <S><R><W>
	0b011, // <S><W><R>
	0b100, // <R><S><W>
	0b101, // <R><W><S>
];
/// Writer Permutations that map from Permutation to Permutation
/* const WRITER_SWAP_PERMUTATIONS: &[u8; 6] = &[
	0b011, // <S><W><R>
	0b010, // <S><R><W>
	0b001, // <W><R><S>
	0b000, // <W><S><R>
	0b101, // <R><W><S>
	0b100, // <R><S><W>
]; */
/// Writer State Map, XOR with state to represent write
const WRITER_STATE_MAP: &[usize; 16] = &[
	0b1011,     // 0b000 -> 0b011
	0b1011,     // 0b001 -> 0b010
	0b1011,     // 0b010 -> 0b001
	0b1011,     // 0b011 -> 0b000
	0b1001,     // 0b100 -> 0b101
	0b1001,     // 0b101 -> 0b100
	0b11111111, // (Invalid State)
	0b11111111, // (Invalid State)
	0b0011,     // 0b000 -> 0b011
	0b0011,     // 0b001 -> 0b010
	0b0011,     // 0b010 -> 0b001
	0b0011,     // 0b011 -> 0b000
	0b0001,     // 0b100 -> 0b101
	0b0001,     // 0b101 -> 0b100
	0b11111111, // (Invalid State)
	0b11111111, // (Invalid State)
];
const WRITER_CUP_MAP: &[usize; 16] = &[0, 0, 2, 1, 2, 1, 3, 3, 0, 0, 2, 1, 2, 1, 3, 3];

/* const READER_SWAP_PERMUTATIONS: &[u8; 6] = &[
	0b001, // <W><R><S>
	0b000, // <W><S><R>
	0b100, // <R><S><W>
	0b101, // <R><W><S>
	0b010, // <S><R><W>
	0b011, // <S><W><R>
]; */
/// Reader State Map, XOR with state to represent read
const READER_STATE_MAP: &[usize; 16] = &[
	0b0000,     // (Preserve State)
	0b0000,     // (Preserve State)
	0b0000,     // (Preserve State)
	0b0000,     // (Preserve State)
	0b0000,     // (Preserve State)
	0b0000,     // (Preserve State)
	0b11111111, // (Invalid State)
	0b11111111, // (Invalid State)
	0b1001,     // 0b000 -> 0b001
	0b1001,     // 0b001 -> 0b000
	0b1110,     // 0b010 -> 0b100
	0b1110,     // 0b011 -> 0b101
	0b1110,     // 0b100 -> 0b010
	0b1110,     // 0b101 -> 0b011
	0b11111111, // (Invalid State)
	0b11111111, // (Invalid State)
];
const READER_CUP_MAP: &[usize; 16] = &[2, 1, 1, 2, 0, 0, 3, 3, 2, 1, 1, 2, 0, 0, 3, 3];

/// A simple async channel used to quickly update data between threads
/// Useful in a situation where you need to model some read-only state on a receiving thread that can be periodically, but quickly, updated from a writer thread.
struct Cupchan<T> {
	// One of these cups is reading, one writing, one for intermediate storage, which one is which depends on the permutation state
	cups: [UnsafeCell<T>; 3], // Use boxes to avoid False Sharing between cpu cache lines https://en.wikipedia.org/wiki/False_sharing
	/// Represents the permutation of cups i.e. which one is the reader, writer, and storage as well as whether or not storage is ready to be read from.
	state: AtomicUsize,
	/// True if reader or writer is dropped
	unconnected: AtomicBool,
}
/// Create a new Cup Channel
pub fn cupchan<T: Clone>(initial: T) -> (CupchanWriter<T>, CupchanReader<T>) {
	let chan = Cupchan {
		cups: [
			UnsafeCell::new(initial.clone()),
			UnsafeCell::new(initial.clone()),
			UnsafeCell::new(initial),
		],
		state: AtomicUsize::new(OBJECT_PERMUTATIONS[0]), // Initial state: <W><S><R> permutation with UPDATE_FLAG unset
		unconnected: AtomicBool::new(false),
	};
	let chan = Box::leak(Box::new(chan)); // Use special dropping logic based on self.unconnected
	(
		CupchanWriter {
			chan,
			current_cup: &chan.cups[0],
		},
		CupchanReader { chan },
	)
}
impl<T: fmt::Debug> fmt::Debug for Cupchan<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("Cupchan")
			.field("cups", &self.cups)
			.field("state", &self.state.load(Ordering::SeqCst))
			.field("unconnected", &self.unconnected.load(Ordering::SeqCst))
			.finish()
	}
}

/// Write to the Cup Channel, make sure to call flush() afterwards.
#[derive(Debug)]
pub struct CupchanWriter<T: 'static> {
	chan: &'static Cupchan<T>,
	current_cup: &'static UnsafeCell<T>,
}
impl<T> CupchanWriter<T> {
	fn new(chan: &'static Cupchan<T>) -> Self {
		let cup_index = WRITER_CUP_MAP[chan.state.load(Ordering::Acquire)];
		Self {
			chan,
			current_cup: &chan.cups[cup_index],
		}
	}
	pub fn flush(&mut self) {
		// Needs exclusive reference
		// Update storage flag & swap cups
		let res = self
			.chan
			.state
			.fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
				Some(state ^ WRITER_STATE_MAP[state])
			})
			.unwrap();
		self.current_cup = &self.chan.cups[WRITER_CUP_MAP[res ^ WRITER_STATE_MAP[res]]];
	}
	pub fn new_reader(&self) -> Option<CupchanReader<T>> {
		// Set unconnected false, If was actually unconnected, return new reader
		if self.chan.unconnected.swap(false, Ordering::SeqCst) {
			Some(CupchanReader::new(self.chan))
		} else {
			None
		}
	}

	#[cfg(loom)]
	pub fn loom_ptr(&mut self) -> MutPtr<T> {
		self.current_cup.get_mut()
	}
}
#[cfg(not(loom))]
impl<T> Deref for CupchanWriter<T> {
	type Target = T;
	fn deref(&self) -> &Self::Target {
		unsafe { &*self.current_cup.get() }
	}
}
#[cfg(not(loom))]
impl<T> DerefMut for CupchanWriter<T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		unsafe { &mut *self.current_cup.get() }
	}
}
impl<T> Drop for CupchanWriter<T> {
	fn drop(&mut self) {
		// Set unconnected to true
		let was_unconnected = self.chan.unconnected.swap(true, Ordering::AcqRel);
		if was_unconnected {
			// If was unconnected, drop channel
			unsafe {
				let ptr = std::mem::transmute::<_, *mut Cupchan<T>>(self.chan);
				ptr::drop_in_place(ptr);
			}
		}
	}
}
// Allow sending between threads
unsafe impl<T: Sync + Send> Send for CupchanWriter<T> {}
unsafe impl<T: Sync + Send> Sync for CupchanWriter<T> {}

// when created, modify state to set reader lock flag
// when dropped, modify state permutation to swap reader & storage object, unset reader lock flag, unset storage new flag
/// Read from the Cup Channel by dereferencing this obejct

#[derive(Debug)]
pub struct CupchanReader<T: 'static> {
	chan: &'static Cupchan<T>,
}
impl<T> CupchanReader<T> {
	fn new(chan: &'static Cupchan<T>) -> Self {
		Self { chan }
	}
	#[inline]
	fn read(&self) -> &'static UnsafeCell<T> {
		let res = self
			.chan
			.state
			.fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
				Some(state ^ READER_STATE_MAP[state])
			})
			.unwrap();
		&self.chan.cups[READER_CUP_MAP[res ^ READER_STATE_MAP[res]]]
	}
	pub fn new_writer(&self) -> Option<CupchanWriter<T>> {
		// Set unconnected false, If was actually unconnected, return new reader
		if self.chan.unconnected.swap(false, Ordering::SeqCst) {
			Some(CupchanWriter::new(self.chan))
		} else {
			None
		}
	}
	#[cfg(loom)]
	pub fn loom_ptr(&self) -> ConstPtr<T> {
		self.read().get()
	}
}
#[cfg(not(loom))]
impl<T> Deref for CupchanReader<T> {
	type Target = T;
	fn deref(&self) -> &Self::Target {
		unsafe { &(*self.read().get()) }
	}
}
impl<T> Drop for CupchanReader<T> {
	fn drop(&mut self) {
		// Set unconnected to true
		let was_unconnected = self.chan.unconnected.swap(true, Ordering::AcqRel);
		if was_unconnected {
			// If was unconnected, drop channel
			unsafe {
				let ptr = std::mem::transmute::<_, *mut Cupchan<T>>(self.chan);
				ptr::drop_in_place(ptr);
			}
		}
	}
}
// Allow sending between threads
unsafe impl<T: Sync + Send> Send for CupchanReader<T> {}
unsafe impl<T: Sync + Send> Sync for CupchanReader<T> {}

#[cfg(test)]
mod tests {
	extern crate test;
	use test::Bencher;

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

	const MAX: usize = 5_000;
	#[test]
	fn cupchan_async_greedy_reader() {
		let (mut writer, reader) = cupchan(0usize);

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
	#[test]
	fn cupchan_async_lazy_reader() {
		let (mut writer, reader) = cupchan(0usize);

		let join = thread::spawn(move || {
			for i in 0..MAX {
				*writer = i;
				writer.flush();
			}
		});

		let mut current = *reader;
		// let mut array = [0usize; MAX];
		while current < MAX - 1 {
			thread::yield_now();
			current = *reader;
		}
		// print!("{:?}", array);
		assert!(*reader == MAX - 1);

		join.join().unwrap();
	}

	#[test]
	fn crossbeam_chan_async() {
		let (tx, rx) = crossbeam_channel::bounded(3);

		let join = thread::spawn(move || {
			for i in 0..MAX {
				tx.send(i).unwrap();
			}
		});

		let mut current = 0;
		for _ in 0..MAX {
			current = rx.recv().unwrap();
		}
		assert!(current == MAX - 1);

		join.join().unwrap();
	}

	#[test]
	fn crossbeam_chan_async_cap_10() {
		let (tx, rx) = crossbeam_channel::bounded(10);

		let join = thread::spawn(move || {
			for i in 0..MAX {
				tx.send(i).unwrap();
			}
		});

		let mut current = 0;
		for _ in 0..MAX {
			current = rx.recv().unwrap();
		}
		assert!(current == MAX - 1);

		join.join().unwrap();
	}

	#[test]
	fn flume_chan_async() {
		let (tx, rx) = flume::unbounded();

		let join = thread::spawn(move || {
			for i in 0..MAX {
				tx.send(i).unwrap();
			}
		});

		let mut current = 0;
		for _ in 0..MAX {
			current = rx.recv().unwrap();
		}
		assert!(current == MAX - 1);

		join.join().unwrap();
	}

	#[bench]
	fn bench_cupchan_greedy(b: &mut Bencher) {
		b.iter(|| {
			cupchan_async_greedy_reader();
		})
	}
	#[bench]
	fn bench_cupchan_lazy(b: &mut Bencher) {
		b.iter(|| {
			cupchan_async_lazy_reader();
		})
	}

	#[bench]
	fn bench_crossbeam_chan_cap_3(b: &mut Bencher) {
		b.iter(|| {
			crossbeam_chan_async();
		})
	}

	#[bench]
	fn bench_crossbeam_chan_cap_10(b: &mut Bencher) {
		b.iter(|| {
			crossbeam_chan_async_cap_10();
		})
	}

	#[bench]
	fn bench_flume_chan(b: &mut Bencher) {
		b.iter(|| {
			flume_chan_async();
		})
	}
}
