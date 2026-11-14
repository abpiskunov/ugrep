use std::path::PathBuf;

// The retained C codec libraries (zlib, bzip2, xz, lz4, zstd, viiz) are called through `extern "C"` by the Rust zstream
// codecs. They are built outside the repository by migration/harness/zstream/fetch-codecs.ps1 (static, /MT, x64) into
// `<repo>\..\codec-build\lib`; CPP2RUST_CODEC_LIB names another directory. As static libraries they are bundled into
// the Rust library, so the umbrella lib and the unit tests resolve them.
const LIBS: [&str; 6] = ["zlib", "bz2", "lzma", "lz4", "zstd", "viiz"];

fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=codec_stubs.cpp");
    println!("cargo:rerun-if-env-changed=CPP2RUST_CODEC_LIB");
    let target_msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let mut bridge = cxx_build::bridge("src/lib.rs");
    bridge.std("c++14");

    let dir = match std::env::var_os("CPP2RUST_CODEC_LIB") {
        Some(d) => PathBuf::from(d),
        None => {
            let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
            // <repo>/rust/src_leaves -> <repo>/../codec-build/lib
            manifest.join("..").join("..").join("..").join("codec-build").join("lib")
        }
    };
    let file_of = |lib: &str| if target_msvc { format!("{lib}.lib") } else { format!("lib{lib}.a") };
    let all_found = dir.is_dir() && LIBS.iter().all(|l| dir.join(file_of(l)).is_file());
    if !all_found {
        // Without the codec libraries (the product build) link-time stand-ins keep the umbrella linkable.
        println!("cargo:warning=codec libraries not found in {} (run migration/harness/zstream/fetch-codecs.ps1); linking stubs", dir.display());
        bridge.file("codec_stubs.cpp");
        bridge.compile("src_leaves_bridge");
        return;
    }
    bridge.compile("src_leaves_bridge");
    println!("cargo:rerun-if-changed={}", dir.display());
    println!("cargo:rustc-link-search=native={}", dir.display());
    for lib in LIBS {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    // The libraries are compiled /MT and the Rust test executables use the dynamic CRT.
    if target_msvc {
        println!("cargo:rustc-link-arg=/NODEFAULTLIB:libcmt.lib");
    }
}
