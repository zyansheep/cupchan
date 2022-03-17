use std::{mem::size_of, ops::{Deref, DerefMut}, sync::atomic::{AtomicU8, Ordering}};

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
	0b100, // <R><S><W>
];
const PERMUTATION_MASK: u8 = 0b00000111;

const WRITER_LOCK: u8 = 0b10000000; // XOR with state to toggle
const READER_LOCK: u8 = 0b01000000; // XOR with state to toggle
const UPDATE_FLAG: u8 = 0b00100000; // OR with state to set


#[allow(unused)]
pub struct Dupex<T> {
	// One of these objects is reading, one writing, one for intermediate storage.
	objects: [T; 3],
	// least significant bits = represents the permutation of objects (of which there are 6) i.e. which one is the reader, writer, and storage
	// 3 bit = writer is locked, 4 bit = reader is locked
	// 5 bit, is storage new
	state: AtomicU8, // 0b00000000
}
impl<T: Clone> Dupex<T> {
	pub fn new(initial: T) -> (DupexWriter<T>, DupexReader<T>) {
		let dupex = Dupex {
			objects: [initial.clone(), initial.clone(), initial],
			state: AtomicU8::new(OBJECT_PERMUTATIONS[0] & WRITER_LOCK & READER_LOCK), // Initial state: read & write are locked, <W><S><R> permutation
		};
		let ptr = Box::into_raw(Box::new(dupex)); // Use special dropping logic based on state
		(DupexWriter { ptr }, DupexReader { ptr })
	}
}

// when created, modify state to set writer lock flag
// when dropped, modify permutation to swap writer & storage object, unset writer lock flag, set storage new flag
pub struct DupexWriter<T> {
	ptr: *mut Dupex<T>, // 
}
impl<T> DupexWriter<T> {
	pub fn write(&mut self) { // Needs exclusive reference
		let state = unsafe { &(*self.ptr).state };
		// Update storage flag & swap objects
		let _ = state.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |state| {
			Some(state ^ WRITER_SWAP_PERMUTATIONS[state as usize] & UPDATE_FLAG)
		});
	}
}
impl<T> Deref for DupexWriter<T> {
	type Target = T;
	fn deref(&self) -> &Self::Target {
		let state = unsafe { (&*self.ptr).state.load(Ordering::SeqCst) };
		let state = state & PERMUTATION_MASK; // get permutation
		let write_obj = WRITER_PERMUTATIONS[state as usize];
		unsafe { &*self.ptr.add(write_obj * size_of::<T>()).cast::<T>() }
	}
}
impl<T> DerefMut for DupexWriter<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let state = unsafe { (&*self.ptr).state.load(Ordering::SeqCst) };
		let state = state & PERMUTATION_MASK; // get permutation
		let write_obj = WRITER_PERMUTATIONS[state as usize];
		unsafe { &mut *self.ptr.add(write_obj * size_of::<T>()).cast::<T>() }
    }
}
impl<T> Drop for DupexWriter<T> {
	fn drop(&mut self) {
		let state = unsafe { &(*self.ptr).state };
		if let Err(_) = state.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |state| {
			if state & READER_LOCK == 0 { // if reader lock is gone, return none
				None
			} else {
				Some(state ^ WRITER_LOCK) // Toggle writer lock
			}
		}) {
			let _ = unsafe { Box::from_raw(self.ptr) }; // put it in a box and consign it to the void
		}
	}
}

// when created, modify state to set reader lock flag
// when dropped, modify state permutation to swap reader & storage object, unset reader lock flag, unset storage new flag
pub struct DupexReader<T> { 
	ptr: *mut Dupex<T>, 
}
impl<T> DupexReader<T> {
	pub fn read(&mut self) { // Needs exclusive reference
		let state = unsafe { &(*self.ptr).state };
		// Update storage flag & swap objects
		let _ = state.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |state| {
			if state & UPDATE_FLAG == 1 {
				Some((state & !PERMUTATION_MASK) ^ READER_SWAP_PERMUTATIONS[state as usize])
			} else {
				None
			}
		});
	}
}
impl<T> Deref for DupexReader<T> {
	type Target = T;
	fn deref(&self) -> &Self::Target {
		let state = unsafe { (&*self.ptr).state.load(Ordering::SeqCst) };
		let state = state & PERMUTATION_MASK; // get permutation
		let write_obj = READER_PERMUTATIONS[state as usize];
		unsafe { &*self.ptr.add(write_obj * size_of::<T>()).cast::<T>() }
	}
}
impl<T> Drop for DupexReader<T> {
    fn drop(&mut self) {
        let state = unsafe { &(*self.ptr).state };
		if let Err(_) = state.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |state| {
			if state & WRITER_LOCK == 0 { // if writer lock is gone, return none
				None
			} else {
				Some(state ^ WRITER_LOCK) // Toggle writer lock
			}
		}) {
			let _ = unsafe { Box::from_raw(self.ptr) }; // put it in a box and consign it to the void
		}
    }
}


#[cfg(test)]
mod tests {
	#[test]
	fn it_works() {
		let result = 2 + 2;
		assert_eq!(result, 4);
	}
}
