use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Check if this is a publish/verify build by detecting if we're in the extracted package
    // The extracted package won't have the install/ directory or deps/ submodule
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let is_local_build = manifest_dir.join("deps/mooncake/.git").exists() || 
                         manifest_dir.join("Makefile").exists();
    
    if !is_local_build {
        println!("cargo:warning=Package build detected - skipping C++ library linking");
        // For published crate, just verify Rust code compiles
        // Users need to link C++ libs themselves
        return;
    }

    // Try to find Mooncake via pkg-config first
    if let Ok(lib) = pkg_config::probe_library("mooncake") {
        println!("cargo:warning=Found Mooncake via pkg-config");
        for path in &lib.link_paths {
            println!("cargo:rustc-link-search=native={}", path.display());
        }
        for lib_name in &lib.libs {
            println!("cargo:rustc-link-lib={}", lib_name);
        }
        return;
    }

    // Check for local install directory from Makefile build
    let local_install = manifest_dir.join("install");
    if local_install.join("lib/libtransfer_engine.a").exists() {
        println!("cargo:warning=Using local install at {:?}", local_install);
        let mooncake_root = local_install.to_str().unwrap();

        let lib_dir = PathBuf::from(&mooncake_root).join("lib");
        let include_dir = PathBuf::from(&mooncake_root).join("include");

        println!("cargo:rustc-link-search=native={}", lib_dir.display());
        println!("cargo:rustc-link-lib=static=transfer_engine");
        println!("cargo:rustc-link-lib=static=mooncake_common");
        println!("cargo:include={}", include_dir.display());

        // Also link required dependencies
        println!("cargo:rustc-link-lib=ibverbs"); // RDMA verbs
        println!("cargo:rustc-link-lib=rdmacm"); // RDMA connection manager

        return;
    }

    // Fallback: check environment variables
    if let Ok(mooncake_root) = env::var("MOONCAKE_ROOT") {
        println!("cargo:warning=Using MOONCAKE_ROOT={}", mooncake_root);

        let lib_dir = PathBuf::from(&mooncake_root).join("lib");
        let include_dir = PathBuf::from(&mooncake_root).join("include");

        println!("cargo:rustc-link-search=native={}", lib_dir.display());
        println!("cargo:rustc-link-lib=static=transfer_engine");
        println!("cargo:rustc-link-lib=static=mooncake_common");
        println!("cargo:include={}", include_dir.display());

        // Also link required dependencies
        println!("cargo:rustc-link-lib=ibverbs"); // RDMA verbs
        println!("cargo:rustc-link-lib=rdmacm"); // RDMA connection manager

        return;
    }

    // Final fallback: assume system paths
    println!("cargo:warning=Mooncake not found via pkg-config or local install, assuming system installation");
    println!("cargo:rustc-link-lib=static=transfer_engine");
    println!("cargo:rustc-link-lib=static=mooncake_common");
    println!("cargo:rustc-link-lib=ibverbs");
    println!("cargo:rustc-link-lib=rdmacm");

    // Add common library paths
    println!("cargo:rustc-link-search=native=/usr/local/lib");
    println!("cargo:rustc-link-search=native=/usr/lib");
    println!("cargo:rustc-link-search=native=/opt/mooncake/lib");
}
