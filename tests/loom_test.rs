#![cfg(loom)]
use std::mem::ManuallyDrop;

use cupchan::{Cupchan, CupchanWriter};
use loom::{thread};

#[test]
fn loom_test() {
	loom::model(|| {
		let (mut writer, reader) = Cupchan::new(0);
		let mut writer = ManuallyDrop::new(writer);
		let reader = ManuallyDrop::new(reader);

		thread::spawn(move || {
			let ptr = writer.loom_ptr();
			unsafe { *ptr.deref() = i; }
			drop(ptr);
			writer.flush();
			thread::yield_now();
		});
		
		const MAX: usize = 3;
		let join = thread::spawn(move || {
			for i in 0..MAX {
				
			}
		});

		
		let mut current = 0;
		let mut array = [0usize; MAX];
		while current < MAX - 1 {
			let ptr = reader.loom_ptr();
			let read = unsafe { *ptr.deref() };
			drop(ptr);
			assert!(current <= read && read < MAX);
			array[read] += 1;
			current = read;
			thread::yield_now();
		}
		print!("{:?}", array);
		assert!(current == MAX - 1);

		join.join().unwrap();
	});
}