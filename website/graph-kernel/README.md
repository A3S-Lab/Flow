# Playground graph kernel

This small Rust crate owns the numeric dependency-layout kernel used by the
A3S Flow Playground. The browser runs the compiled WebAssembly module inside a
dedicated Worker so topological traversal and coordinate assignment do not
block pointer or keyboard interaction on the main thread.

Rebuild the checked-in browser package after changing the Rust source:

```bash
npm run build:graph-kernel --prefix website
```

Run the native unit tests from this directory:

```bash
cargo test --locked
```
