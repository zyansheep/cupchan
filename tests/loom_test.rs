#![cfg(loom)]
use cupchan::{Cupchan, CupchanWriter};
use loom::{thread};

#[test]
fn loom_test() {
	loom::model(|| {
		let (mut writer, reader) = Cupchan::new(0);
		
		const MAX: usize = 20;
		let join = thread::spawn(move || {
			for i in 0..MAX {
				*writer = i;
				writer.flush();
				thread::yield_now();
			}
		});

		
		let mut current = *reader;
		let mut array = [0usize; MAX];
		while current < MAX - 1 {
			let read = *reader;
			assert!(current <= read && read < MAX);
			array[read] += 1;
			current = read;
			thread::yield_now();
		}
		print!("{:?}", array);
		assert!(*reader == MAX - 1);

		join.join().unwrap();
	});
}