use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Try pkg-config first
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

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Check if submodule exists - if so, build from source
    let mooncake_dir = manifest_dir.join("deps/mooncake");
    if mooncake_dir.join("CMakeLists.txt").exists() {
        println!("cargo:warning=Building mooncake from source...");
        build_mooncake_from_source(&mooncake_dir, &out_dir);
        return;
    }

    // Check local install
    let local_install = manifest_dir.join("install");
    if local_install.join("lib/libtransfer_engine.a").exists() {
        println!("cargo:warning=Using local mooncake install");
        link_local(&local_install);
        return;
    }

    // Check MOONCAKE_ROOT
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
            println!("cargo:warning=Using system mooncake");
            println!("cargo:rustc-link-search=native={}", path);
            link_libs();
            return;
        }
    }

    panic!("Mooncake not found. Run: git submodule update --init && make");
}

fn build_mooncake_from_source(mooncake_dir: &PathBuf, out_dir: &PathBuf) {
    // Use a unique build dir based on mooncake_dir path to avoid cache conflicts
    let build_dir = out_dir.join(format!("mooncake-build-{}", hash_path(mooncake_dir)));
    let install_dir = out_dir.join("mooncake-install");

    // Clean build dir if it exists (fresh build)
    let _ = std::fs::remove_dir_all(&build_dir);
    std::fs::create_dir_all(&build_dir).unwrap();
    std::fs::create_dir_all(&install_dir).unwrap();

    // Init submodules if needed
    if !mooncake_dir.join("extern/pybind11/CMakeLists.txt").exists() {
        println!("cargo:warning=Initializing git submodules...");
        run(Command::new("git").args(&[
            "-C",
            mooncake_dir.to_str().unwrap(),
            "submodule",
            "update",
            "--init",
            "--recursive",
        ]));
    }

    // Configure with cmake
    println!("cargo:warning=Configuring mooncake with cmake...");
    run(Command::new("cmake").current_dir(&build_dir).args(&[
        "-DCMAKE_BUILD_TYPE=Release",
        &format!("-DCMAKE_INSTALL_PREFIX={}", install_dir.display()),
        "-DWITH_TE=ON",
        "-DWITH_STORE=OFF",
        "-DWITH_P2P_STORE=OFF",
        "-DWITH_EP=OFF",
        "-DWITH_RUST_EXAMPLE=OFF",
        "-DWITH_STORE_RUST=OFF",
        mooncake_dir.to_str().unwrap(),
    ]));

    // Build
    println!("cargo:warning=Building mooncake (this may take a while)...");
    let num_jobs = env::var("NUM_JOBS").unwrap_or_else(|_| "4".to_string());
    run(Command::new("cmake").current_dir(&build_dir).args(&[
        "--build",
        ".",
        "--parallel",
        &num_jobs,
    ]));

    // Copy libs to install dir
    let lib_dir = install_dir.join("lib");
    std::fs::create_dir_all(&lib_dir).unwrap();

    let te_lib = build_dir.join("mooncake-transfer-engine/src/libtransfer_engine.a");
    let common_lib = build_dir.join("mooncake-common/src/libmooncake_common.a");

    if te_lib.exists() {
        std::fs::copy(&te_lib, lib_dir.join("libtransfer_engine.a")).unwrap();
    }
    if common_lib.exists() {
        std::fs::copy(&common_lib, lib_dir.join("libmooncake_common.a")).unwrap();
    }

    println!("cargo:warning=Mooncake built successfully");
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    link_libs();
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

fn run(cmd: &mut Command) {
    let status = cmd.status().expect("Failed to execute command");
    if !status.success() {
        panic!("Command failed: {:?}", cmd);
    }
}

fn hash_path(path: &PathBuf) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}
