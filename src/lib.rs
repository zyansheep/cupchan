use std::{ops::{Deref, DerefMut}, sync::atomic::{AtomicU8, Ordering}};

const OBJECT_PERMUTATIONS: &'static [u8; 6] = &[
	0b000, // <W><S><R>
	0b001, // <W><R><S>
	0b010, // <S><R><W>
	0b011, // <S><W><R>
	0b100, // <R><S><W>
	0b101, // <R><W><S>
];
const WRITER_PERMUTATIONS: &'static [usize; 6] = &[
	0, 0, 2, 1, 2, 1
];
const READER_PERMUTATIONS: &'static [usize; 6] = &[
	2, 1, 1, 2, 0, 0
];
const WRITER_SWAP_PERMUTATIONS: &'static [u8; 6] = &[
	0b011, // <S><W><R>
	0b010, // <S><R><W>
	0b001, // <W><R><S>
	0b000, // <W><S><R>
	0b101, // <R><W><S>
	0b100, // <R><S><W>
];
const READER_SWAP_PERMUTATIONS: &'static [u8; 6] = &[
	0b001, // <W><R><S>
	0b000, // <W><S><R>
	0b100, // <R><S><W>
	0b101, // <R><W><S>
	0b010, // <S><R><W>
	0b011, // <S><W><R>
];
const PERMUTATION_MASK: u8 = 0b00000111;
const LOCK_MASK: 		u8 = 0b11000000;

const WRITER_LOCK: u8 = 0b10000000; // XOR with state to toggle
const READER_LOCK: u8 = 0b01000000; // XOR with state to toggle
const UPDATE_FLAG: u8 = 0b00100000; // OR with state to set

/// A simple channel used to quickly update data between threads
#[derive(Debug)]
pub struct Cupchan<T> {
	// One of these objects is reading, one writing, one for intermediate storage.
	objects: [T; 3],
	// least significant bits = represents the permutation of objects (of which there are 6) i.e. which one is the reader, writer, and storage
	// most significant bits = (writer lock, reader lock, whether storage was just updated)
	state: AtomicU8,
}
impl<T: Clone> Cupchan<T> {
	pub fn new(initial: T) -> (CupchanWriter<T>, CupchanReader<T>) {
		let chan = Cupchan {
			objects: [initial.clone(), initial.clone(), initial],
			state: AtomicU8::new(OBJECT_PERMUTATIONS[0] ^ WRITER_LOCK ^ READER_LOCK), // Initial state: read & write are locked, <W><S><R> permutation
		};
		let ptr = Box::into_raw(Box::new(chan)); // Use special dropping logic based on state
		(CupchanWriter { ptr }, CupchanReader { ptr })
	}
}

// when created, modify state to set writer lock flag
// when dropped, modify permutation to swap writer & storage object, unset writer lock flag, set storage new flag
pub struct CupchanWriter<T> {
	ptr: *mut Cupchan<T>, // 
}
impl<T> CupchanWriter<T> {
	pub fn flush(&mut self) { // Needs exclusive reference
		let state = unsafe { &(*self.ptr).state };
		// Update storage flag & swap objects
		let _ = state.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |state| {
			let res = (state & !PERMUTATION_MASK) ^ WRITER_SWAP_PERMUTATIONS[(state & PERMUTATION_MASK) as usize] ^ UPDATE_FLAG;
			Some(res)
		});
	}
	pub fn print(&self)
	where T: std::fmt::Debug {
		let chan = unsafe { &(*self.ptr) };
		println!("write state: {:?}, {:0>8b}", &chan.objects, chan.state.load(Ordering::SeqCst));
	}
}
impl<T> Deref for CupchanWriter<T> {
	type Target = T;
	fn deref(&self) -> &Self::Target {
		let ptr = unsafe { &*self.ptr };
		let state = ptr.state.load(Ordering::SeqCst);
		let obj_idx = WRITER_PERMUTATIONS[(state & PERMUTATION_MASK) as usize];
		&ptr.objects[obj_idx]
	}
}
impl<T> DerefMut for CupchanWriter<T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		let ptr = unsafe { &mut *self.ptr };
		let state = ptr.state.load(Ordering::SeqCst);
		let obj_idx = WRITER_PERMUTATIONS[(state & PERMUTATION_MASK) as usize];
		&mut ptr.objects[obj_idx]
	}
}
impl<T> Drop for CupchanWriter<T> {
	fn drop(&mut self) {
		let state = unsafe { &(*self.ptr).state };
		let state = state.fetch_xor(WRITER_LOCK, Ordering::SeqCst);
		if state & LOCK_MASK == 0 {
			let _ = unsafe { Box::from_raw(self.ptr) }; // put it in a box and consign it to the void
		}
	}
}

// when created, modify state to set reader lock flag
// when dropped, modify state permutation to swap reader & storage object, unset reader lock flag, unset storage new flag
pub struct CupchanReader<T> { 
	ptr: *mut Cupchan<T>, 
}
impl<T> CupchanReader<T> {
	pub fn print(&self)
	where T: std::fmt::Debug {
		let chan = unsafe { &(*self.ptr) };
		println!("read state: {:?}, {:0>8b}", &chan.objects, chan.state.load(Ordering::SeqCst));
	}
}
impl<T> Deref for CupchanReader<T> {
	type Target = T;
	fn deref(&self) -> &Self::Target {
		let state = unsafe { &(*self.ptr).state };
		let _ = state.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |state| {
			if state & UPDATE_FLAG != 0 {
				let ret = (state & !PERMUTATION_MASK) ^ READER_SWAP_PERMUTATIONS[(state & PERMUTATION_MASK) as usize] ^ UPDATE_FLAG;
				Some(ret)
			} else {
				None
			}
		});
		let state = state.load(Ordering::SeqCst);
		let obj_idx = READER_PERMUTATIONS[(state & PERMUTATION_MASK) as usize];
		unsafe { &(*self.ptr).objects[obj_idx] }
	}
}
impl<T> Drop for CupchanReader<T> {
	fn drop(&mut self) {
		let state = unsafe { &(*self.ptr).state };
		let state = state.fetch_xor(READER_LOCK, Ordering::SeqCst);
		if state & LOCK_MASK == 0 {
			let _ = unsafe { Box::from_raw(self.ptr) }; // put it in a box and consign it to the void
		}
	}
}


#[cfg(test)]
mod tests {
	use crate::Cupchan;

	#[test]
	fn test_chan() {
		let (mut writer, mut reader) = Cupchan::new(0);
		writer.print();
		reader.print();

		*writer = 1;
		writer.print();
		reader.print();

		writer.flush();
		writer.print();
		reader.print();

		assert_eq!(*reader, 1);
		*writer = 2; writer.flush();
		assert_eq!(*reader, 2);

		drop(reader);

		/* *writer = 2;
		writer.write();
		drop(writer) */
	}
}
