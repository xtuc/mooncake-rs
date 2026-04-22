# Makefile for mooncake-rs with mooncake submodule

# Directories
MOONCAKE_DIR = deps/mooncake
MOONCAKE_BUILD_DIR = $(MOONCAKE_DIR)/build
INSTALL_PREFIX = $(PWD)/install
TE_SRC_DIR = $(MOONCAKE_DIR)/mooncake-transfer-engine

# CMake options
CMAKE_OPTIONS = -DCMAKE_BUILD_TYPE=Release \
                -DCMAKE_INSTALL_PREFIX=$(INSTALL_PREFIX) \
                -DWITH_TE=ON \
                -DWITH_STORE=OFF \
                -DWITH_P2P_STORE=OFF \
                -DWITH_EP=OFF \
                -DWITH_RUST_EXAMPLE=OFF \
                -DWITH_STORE_RUST=OFF

# Phony targets
.PHONY: all clean mooncake rust install

# Default target builds everything
all: mooncake rust

# Build mooncake C++ library
mooncake:
	@echo "Building Mooncake Transfer Engine..."
	@mkdir -p $(MOONCAKE_BUILD_DIR)
	@cd $(MOONCAKE_BUILD_DIR) && cmake $(CMAKE_OPTIONS) ..
	@cd $(MOONCAKE_BUILD_DIR) && $(MAKE) -j$$(nproc)
	@mkdir -p $(INSTALL_PREFIX)/lib $(INSTALL_PREFIX)/include
	@cp $(MOONCAKE_BUILD_DIR)/mooncake-transfer-engine/src/libtransfer_engine.a $(INSTALL_PREFIX)/lib/
	@cp $(MOONCAKE_BUILD_DIR)/mooncake-common/src/libmooncake_common.a $(INSTALL_PREFIX)/lib/
	@cp -r $(TE_SRC_DIR)/include/* $(INSTALL_PREFIX)/include/
	@echo "Mooncake installed to $(INSTALL_PREFIX)"

# Build Rust project with mooncake linked
rust: mooncake
	@echo "Building Rust project..."
	@MOONCAKE_ROOT=$(INSTALL_PREFIX) cargo build --release

# Install everything
install: mooncake
	@echo "Installing to $(INSTALL_PREFIX)..."
	@cd $(MOONCAKE_BUILD_DIR) && $(MAKE) install

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	@rm -rf $(MOONCAKE_BUILD_DIR)
	@rm -rf $(INSTALL_PREFIX)
	@cargo clean

# Development build (debug)
dev:
	@echo "Building Mooncake (debug)..."
	@mkdir -p $(MOONCAKE_BUILD_DIR)
	@cd $(MOONCAKE_BUILD_DIR) && cmake -DCMAKE_BUILD_TYPE=Debug \
		-DCMAKE_INSTALL_PREFIX=$(INSTALL_PREFIX) \
		-DWITH_TE=ON -DWITH_STORE=OFF -DWITH_P2P_STORE=OFF \
		-DWITH_EP=OFF -DWITH_RUST_EXAMPLE=OFF -DWITH_STORE_RUST=OFF ..
	@cd $(MOONCAKE_BUILD_DIR) && $(MAKE) -j$$(nproc)
	@mkdir -p $(INSTALL_PREFIX)/lib $(INSTALL_PREFIX)/include
	@cp $(MOONCAKE_BUILD_DIR)/mooncake-transfer-engine/src/libtransfer_engine.a $(INSTALL_PREFIX)/lib/
	@cp $(MOONCAKE_BUILD_DIR)/mooncake-common/src/libmooncake_common.a $(INSTALL_PREFIX)/lib/
	@cp -r $(TE_SRC_DIR)/include/* $(INSTALL_PREFIX)/include/
	@MOONCAKE_ROOT=$(INSTALL_PREFIX) cargo build

# Quick rebuild (skip cmake config)
rebuild:
	@cd $(MOONCAKE_BUILD_DIR) && $(MAKE) -j$$(nproc)
	@mkdir -p $(INSTALL_PREFIX)/lib $(INSTALL_PREFIX)/include
	@cp $(MOONCAKE_BUILD_DIR)/mooncake-transfer-engine/src/libtransfer_engine.a $(INSTALL_PREFIX)/lib/
	@cp $(MOONCAKE_BUILD_DIR)/mooncake-common/src/libmooncake_common.a $(INSTALL_PREFIX)/lib/
	@cp -r $(TE_SRC_DIR)/include/* $(INSTALL_PREFIX)/include/
	@MOONCAKE_ROOT=$(INSTALL_PREFIX) cargo build --release

# Prepare for publish (build with local libs, then verify)
publish: mooncake
	@echo "Preparing for crates.io publish..."
	@echo "Building with local mooncake libs..."
	@MOONCAKE_ROOT=$(INSTALL_PREFIX) cargo build --release
	@echo "Verifying package..."
	@cargo publish --dry-run --allow-dirty
	@echo "Ready to publish. Run: cargo publish"
