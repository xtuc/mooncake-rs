use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=MOONCAKE_ROOT");
    println!("cargo:rerun-if-env-changed=TRANSFER_ENGINE_LIB_DIR");

    // Check if this is a local development build
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let is_local_build =
        manifest_dir.join("deps/mooncake/.git").exists() || manifest_dir.join("Makefile").exists();

    // Try pkg-config first (works for both local and published)
    if try_pkg_config() {
        return;
    }

    // Try explicit environment variables (works for both local and published)
    if try_env_vars() {
        return;
    }

    // Try local install directory (only for local development)
    if is_local_build && try_local_install(&manifest_dir) {
        return;
    }

    // Try system paths (works for both local and published)
    if try_system_paths() {
        return;
    }

    // If we're in a package build (crates.io), don't fail - just warn
    // The user needs to install mooncake separately
    if !is_local_build {
        println!(
            "cargo:warning=Mooncake libraries not found. Set MOONCAKE_ROOT or install system-wide."
        );
        return;
    }

    // Not found in local build - this is an error
    print_error_and_panic();
}

fn try_pkg_config() -> bool {
    match pkg_config::probe_library("transfer_engine") {
        Ok(lib) => {
            println!("cargo:warning=Found transfer_engine via pkg-config");
            for path in &lib.link_paths {
                println!("cargo:rustc-link-search=native={}", path.display());
            }
            // pkg-config already prints the libs, but ensure we get RDMA deps
            println!("cargo:rustc-link-lib=ibverbs");
            println!("cargo:rustc-link-lib=rdmacm");
            true
        }
        Err(_) => false,
    }
}

fn try_env_vars() -> bool {
    // Check TRANSFER_ENGINE_LIB_DIR first (most explicit)
    if let Ok(lib_dir) = env::var("TRANSFER_ENGINE_LIB_DIR") {
        let lib_path = PathBuf::from(&lib_dir);
        if let Some(lib_type) = find_library(&lib_path, "transfer_engine") {
            println!("cargo:warning=Using TRANSFER_ENGINE_LIB_DIR={}", lib_dir);
            println!("cargo:rustc-link-search=native={}", lib_dir);
            link_transfer_engine(lib_type);
            return true;
        }
    }

    // Check MOONCAKE_ROOT
    if let Ok(root) = env::var("MOONCAKE_ROOT") {
        let lib_dir = PathBuf::from(&root).join("lib");
        if let Some(lib_type) = find_library(&lib_dir, "transfer_engine") {
            println!("cargo:warning=Using MOONCAKE_ROOT={}", root);
            println!("cargo:rustc-link-search=native={}", lib_dir.display());
            link_transfer_engine(lib_type);
            return true;
        }

        // Also check for lib64
        let lib64_dir = PathBuf::from(&root).join("lib64");
        if let Some(lib_type) = find_library(&lib64_dir, "transfer_engine") {
            println!("cargo:warning=Using MOONCAKE_ROOT={} (lib64)", root);
            println!("cargo:rustc-link-search=native={}", lib64_dir.display());
            link_transfer_engine(lib_type);
            return true;
        }
    }

    false
}

fn try_local_install(manifest_dir: &PathBuf) -> bool {
    let local_install = manifest_dir.join("install");
    let lib_dir = local_install.join("lib");
    let lib64_dir = local_install.join("lib64");

    if let Some(lib_type) = find_library(&lib_dir, "transfer_engine") {
        println!(
            "cargo:warning=Using local install at {}/install",
            manifest_dir.display()
        );
        println!("cargo:rustc-link-search=native={}", lib_dir.display());
        link_transfer_engine(lib_type);
        return true;
    }

    if let Some(lib_type) = find_library(&lib64_dir, "transfer_engine") {
        println!(
            "cargo:warning=Using local install at {}/install (lib64)",
            manifest_dir.display()
        );
        println!("cargo:rustc-link-search=native={}", lib64_dir.display());
        link_transfer_engine(lib_type);
        return true;
    }

    false
}

fn try_system_paths() -> bool {
    let paths = vec![
        "/usr/local/lib",
        "/usr/local/lib64",
        "/usr/lib",
        "/usr/lib64",
        "/opt/mooncake/lib",
        "/opt/mooncake/lib64",
        "/opt/transfer_engine/lib",
        "/opt/transfer_engine/lib64",
    ];

    for path in &paths {
        let pb = PathBuf::from(path);
        if let Some(lib_type) = find_library(&pb, "transfer_engine") {
            println!("cargo:warning=Using system library at {}", path);
            println!("cargo:rustc-link-search=native={}", path);
            link_transfer_engine(lib_type);
            return true;
        }
    }

    // Also try LD_LIBRARY_PATH
    if let Ok(ld_path) = env::var("LD_LIBRARY_PATH") {
        for path in ld_path.split(':') {
            let pb = PathBuf::from(path);
            if let Some(lib_type) = find_library(&pb, "transfer_engine") {
                println!("cargo:warning=Using LD_LIBRARY_PATH entry: {}", path);
                println!("cargo:rustc-link-search=native={}", path);
                link_transfer_engine(lib_type);
                return true;
            }
        }
    }

    false
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum LibType {
    Static,
    Shared,
}

fn find_library(dir: &PathBuf, name: &str) -> Option<LibType> {
    if !dir.exists() {
        return None;
    }

    // Check for static library first (prefer static)
    let static_lib = dir.join(format!("lib{}.a", name));
    if static_lib.exists() {
        return Some(LibType::Static);
    }

    // Check for shared library (.so)
    let shared_lib = dir.join(format!("lib{}.so", name));
    if shared_lib.exists() {
        return Some(LibType::Shared);
    }

    // Check for versioned shared library
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.starts_with(&format!("lib{}.so.", name)) {
                return Some(LibType::Shared);
            }
        }
    }

    None
}

fn link_transfer_engine(lib_type: LibType) {
    match lib_type {
        LibType::Static => {
            println!("cargo:rustc-link-lib=static=transfer_engine");
            println!("cargo:rustc-link-lib=static=mooncake_common");
        }
        LibType::Shared => {
            println!("cargo:rustc-link-lib=dylib=transfer_engine");
            println!("cargo:rustc-link-lib=dylib=mooncake_common");
            println!("cargo:rustc-link-lib=dylib=asio");
        }
    }

    // RDMA dependencies
    println!("cargo:rustc-link-lib=ibverbs");
    println!("cargo:rustc-link-lib=rdmacm");
}

fn print_error_and_panic() {
    eprintln!("\n========================================");
    eprintln!("ERROR: Mooncake Transfer Engine not found!");
    eprintln!("========================================\n");
    eprintln!("Searched locations:");
    eprintln!("  - pkg-config (transfer_engine)");
    eprintln!("  - TRANSFER_ENGINE_LIB_DIR environment variable");
    eprintln!("  - MOONCAKE_ROOT environment variable");
    eprintln!("  - ./install/lib (local directory)");
    eprintln!("  - /usr/local/lib, /usr/lib, /opt/mooncake/lib");
    eprintln!("  - LD_LIBRARY_PATH\n");
    eprintln!("To install Mooncake:");
    eprintln!("  git clone https://github.com/kvcache-ai/mooncake.git");
    eprintln!("  cd mooncake/mooncake-transfer-engine");
    eprintln!("  mkdir build && cd build");
    eprintln!("  cmake ..");
    eprintln!("  make && sudo make install\n");
    eprintln!("Or set environment variable:");
    eprintln!("  export TRANSFER_ENGINE_LIB_DIR=/path/to/mooncake/lib\n");
    eprintln!("========================================\n");

    panic!("Mooncake Transfer Engine libraries not found");
}
