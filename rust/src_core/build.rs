fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=build.rs");
    // The generated header is `<cargo target>/cxxbridge/src_core/src/lib.rs.h`; msvc/rust.targets adds the
    // `cxxbridge` directory to the include path of the Rust variant (src/core_rust.hpp includes it).
    cxx_build::bridge("src/lib.rs").std("c++14").compile("src_core_bridge");
}
