Build only rlib for static desktop embedding. Upstream also emits a Rust dylib, which requires panic_unwind and conflicts with the desktop panic=abort profile. Conversion code is unchanged.
