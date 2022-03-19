//! note to self: do not use the various LOOM_ flags, they will make loom crash on this test for some reason

#![cfg(loom)]
use cupchan::{cupchan, CupchanWriter};
use loom::thread;

#[test]
fn loom_test() {
    loom::model(|| {
        let (mut writer, reader) = cupchan(0);

        const MAX: usize = 5;
        let join = thread::spawn(move || {
            for i in 0..MAX {
                let ptr = writer.loom_ptr();
                unsafe {
                    *ptr.deref() = i;
                }
                drop(ptr);
                writer.flush();
                thread::yield_now();
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
