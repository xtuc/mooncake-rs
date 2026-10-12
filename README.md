# mooncake-rs

FFI bindings for Mooncake Transfer Engine.

## Building

Initialize Mooncake submodules and run default build:

```bash
git submodule update --init --recursive
make
```

`make` builds Mooncake shared libraries (`libtransfer_engine.so`,
`libmooncake_common.so`, `libasio.so`) under `install/lib`, then builds Rust
crate with `MOONCAKE_ROOT` set. `build.rs` finds and links existing libraries;
it does not compile Mooncake. For Cargo-only builds, first install Mooncake
and set `MOONCAKE_ROOT` to its installation prefix or
`TRANSFER_ENGINE_LIB_DIR` to its library directory. Published crate excludes
`deps/mooncake`, so it also needs separately installed libraries.

`make mooncake` installs Mooncake's yalantinglibs dependency into `INSTALL_PREFIX` (default `./install`) before building the shared libraries. Set `USE_CUDA=ON` for CUDA transfers and `INSTALL_PREFIX=/usr/local` for system libraries.

Set `LD_LIBRARY_PATH=install/lib` when running linked binaries, including
`cargo run`, so loader can find Mooncake and `libasio.so`.

## Dependencies

- libibverbs (RDMA)
- librdmacm (RDMA connection manager)
- CMake 3.16+
- C++ compiler with C++20 support
