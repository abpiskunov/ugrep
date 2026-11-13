fn main() {
    cxx_build::bridge("src/lib.rs").std("c++14").compile("src_leaves_bridge");
    println!("cargo:rerun-if-changed=src/lib.rs");
}
