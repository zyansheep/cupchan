#![cfg(loom)]
use cupchan::{Cupchan, CupchanWriter};
use loom::{thread};

#[test]
fn loom_test() {
	loom::model(|| {
		let (mut writer, reader) = Cupchan::new(0);
		
		const MAX: usize = 5;
		let join = thread::spawn(move || {
			*writer = MAX;
			writer.flush();
		});
		join.join().unwrap();
		assert_eq!(*reader, MAX);
	});
}