use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Check if this is a publish/verify build by detecting if we're in the extracted package
    // The extracted package won't have the install/ directory or deps/ submodule
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let is_local_build =
        manifest_dir.join("deps/mooncake/.git").exists() || manifest_dir.join("Makefile").exists();

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

    // Check local install (for development)
    let local_install = manifest_dir.join("install");
    if local_install.join("lib/libtransfer_engine.a").exists() {
        println!("cargo:warning=Using local mooncake install");
        link_local(&local_install);
        return;
    }

    // Check MOONCAKE_ROOT environment variable
    if let Ok(root) = env::var("MOONCAKE_ROOT") {
        let lib_dir = PathBuf::from(&root).join("lib");
        if lib_dir.join("libtransfer_engine.a").exists() {
            println!("cargo:warning=Using MOONCAKE_ROOT");
            link_local(&PathBuf::from(root));
            return;
        }
    }

    // System paths
    let system_paths = ["/usr/local/lib", "/usr/lib", "/opt/mooncake/lib"];
    for path in &system_paths {
        if PathBuf::from(path).join("libtransfer_engine.a").exists() {
            println!("cargo:warning=Using system mooncake at {}", path);
            println!("cargo:rustc-link-search=native={}", path);
            link_libs();
            return;
        }
    }

    // Not found - provide helpful error
    eprintln!("\n========================================");
    eprintln!("ERROR: Mooncake C++ libraries not found!");
    eprintln!("========================================\n");
    eprintln!("This crate requires Mooncake Transfer Engine C++ libraries.\n");
    eprintln!("Install Mooncake:");
    eprintln!("  git clone https://github.com/kvcache-ai/mooncake.git");
    eprintln!("  cd mooncake && mkdir build && cd build");
    eprintln!("  cmake -DWITH_TE=ON -DWITH_STORE=OFF ..");
    eprintln!("  make && sudo make install\n");
    eprintln!("Or set MOONCAKE_ROOT:");
    eprintln!("  export MOONCAKE_ROOT=/path/to/mooncake/install\n");
    eprintln!("========================================\n");

    panic!("Mooncake libraries not found. See error above.");
}

fn link_local(install_dir: &PathBuf) {
    println!(
        "cargo:rustc-link-search=native={}",
        install_dir.join("lib").display()
    );
    link_libs();
}

fn link_libs() {
    println!("cargo:rustc-link-lib=static=transfer_engine");
    println!("cargo:rustc-link-lib=static=mooncake_common");
    println!("cargo:rustc-link-lib=ibverbs");
    println!("cargo:rustc-link-lib=rdmacm");
}
