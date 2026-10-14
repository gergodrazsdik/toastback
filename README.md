# toastback

Async Windows toasts with callbacks, for WSL and scripts.

Early development: nothing usable yet.

## Building

Built from WSL for Windows (`x86_64-pc-windows-msvc`) with [cargo-xwin](https://github.com/rust-cross/cargo-xwin). Needs rustup (the toolchain is pinned in `rust-toolchain.toml`), clang, lld, llvm and [just](https://github.com/casey/just).

```sh
just build   # target/x86_64-pc-windows-msvc/release/toastback.exe
just run     # build, then run the .exe from WSL
just lint    # rustfmt + clippy
```

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
