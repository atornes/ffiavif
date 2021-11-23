# incomplete - don't use

# `ffiavif` — PNG/JPEG to AVIF converter library with FFI exports

Based on [cavif-rs](https://github.com/kornelski/cavif-rs)

Encoder for AVIF images. Uses [rav1e](//lib.rs/rav1e) and [avif-serialize](https://lib.rs/avif-serialize).

## Compatibility

* Chrome 85+ desktop,
* Chrome on Android 12,
* Firefox 91. Currently Firefox 92 is not supported.

## Building

To build it from source you need:

* Rust 1.52 or later, preferably via [rustup](https://rustup.rs),
* [`nasm`](https://www.nasm.us/) 2.14 or later.

```bash
cargo build ffiavif
```
