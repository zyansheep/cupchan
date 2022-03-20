# cupchan
<p>
    <a href="https://docs.rs/cupchan">
        <img src="https://img.shields.io/docsrs/cupchan.svg" alt="docs.rs">
    </a>
    <a href="https://crates.io/crates/cupchan">
        <img src="https://img.shields.io/crates/v/cupchan.svg" alt="crates.io">
    </a>
</p>

*Yes cup-chan, please swap my cups around uwu*

Simple async overwriting channel between two threads that is wait &amp; block free by swapping cups around

This project came from the need for me to have a thread lazily update some data on another thread without having to wait for mutexes.

# How it works

the way this crate accomplishes a wait/block-free channel is by having three "cups" with swappable labels. Each cup is marked as having a specific purpose i.e. writing, storage, and reading. (This marker is stored in an atomic u8).

The writing thread has access to the writing cup, and the reading thread has access to the cup marked as reading. Once the writing thread is ready to update the reading thread, it writes to its cup and calls `flush()` which switches the writing and storage markers around. (this is a single atomic operation using [`fetch_update`](https://doc.rust-lang.org/std/sync/atomic/struct.AtomicU8.html#method.fetch_update)).

For example, the cups could start out like this: `<W><S><R>`

The writer thread writes something: `<S><W><R>` - the writer marker has now swaped with the storage marker and a flag is set to tell the reader thread that the storage was updated.

The reader thread wants to check the data, so it checks the update flag. If set, the reader swaps the storage and reader markers. Then the reader looks inside the reader cup for the data.

The system of markers ensures that the writing and reading thread never access the same cup at the same time.

Here is a diagram of all the possible cup states and the relations between them: [quiver](https://q.uiver.app/?q=WzAsMTMsWzYsNCwiXFx0ZXh0cm17V1NSfSJdLFs2LDAsIlxcdGV4dHJte1dTUlxcY2hlY2ttYXJrfSJdLFs4LDQsIlxcdGV4dHJte1dSU30iXSxbNiw4LCJcXHRleHRybXtTUldcXGNoZWNrbWFya30iXSxbMiw4LCJcXHRleHRybXtSU1d9Il0sWzIsNCwiXFx0ZXh0cm17UlNXXFxjaGVja21hcmt9Il0sWzMsNiwiXFx0ZXh0cm17U1JXfSJdLFswLDQsIlxcdGV4dHJte1JXU1xcY2hlY2ttYXJrfSJdLFsyLDAsIlxcdGV4dHJte1NXUn0iXSxbNSw2LCJcXHRleHRybXtXUlNcXGNoZWNrbWFya30iXSxbNSwyLCJcXHRleHRybXtTV1JcXGNoZWNrbWFya30iXSxbMywyLCJcXHRleHRybXtSV1N9Il0sWzAsMTBdLFsyLDMsIlciLDFdLFszLDQsIlIiLDFdLFs1LDYsIlIiLDFdLFs0LDcsIlciLDFdLFs3LDgsIlIiLDFdLFs4LDEsIlciLDFdLFsxLDIsIlIiLDFdLFs5LDAsIlIiLDFdLFs5LDMsIlciLDEseyJzdHlsZSI6eyJ0YWlsIjp7Im5hbWUiOiJhcnJvd2hlYWQifX19XSxbMCwxMCwiVyIsMV0sWzEwLDExLCJSIiwxXSxbMSwxMCwiVyIsMSx7InN0eWxlIjp7InRhaWwiOnsibmFtZSI6ImFycm93aGVhZCJ9fX1dLFsxMSw1LCJXIiwxXSxbNiw5LCJXIiwxXSxbNSw3LCJXIiwxLHsic3R5bGUiOnsidGFpbCI6eyJuYW1lIjoiYXJyb3doZWFkIn19fV1d)

# Tests
This crate has been validated with [loom](https://github.com/tokio-rs/loom)

Run tests:
```shell
$ cargo test
$ RUSTFLAGS="--cfg loom" cargo test --test loom_test --release
```
Note to self: If using LOOM flags, make sure to clear checkpoint file after changing code.

# Benchmarks