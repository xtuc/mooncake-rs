# mooncake-rs

FFI bindings for Mooncake Transfer Engine.

## Building Shared Libraries (.so)

To build Mooncake as shared libraries instead of static:

```bash
cd deps/mooncake
mkdir -p build && cd build

cmake \
  -DBUILD_SHARED_LIBS=ON \
  -DCMAKE_BUILD_TYPE=Release \
  -DWITH_TE=ON \
  -DWITH_STORE=OFF \
  -DWITH_P2P_STORE=OFF \
  -DWITH_EP=OFF \
  ..

make -j$(nproc)
```

This produces:
- `build/mooncake-transfer-engine/src/libtransfer_engine.so`
- `build/mooncake-common/src/libmooncake_common.so`

### Installation

```bash
sudo make install
# Or to custom prefix:
cmake -DCMAKE_INSTALL_PREFIX=/path/to/install ..
make && make install
```

### Usage with This Crate

Set the library path:

```bash
export MOONCAKE_ROOT=/path/to/mooncake/install
# Or use system paths
cargo build
```

## Development

Build with local static libs (default):

```bash
make
```

This builds mooncake from source and links statically.

## Dependencies

- libibverbs (RDMA)
- librdmacm (RDMA connection manager)
- CMake 3.16+
- C++ compiler with C++17 support
