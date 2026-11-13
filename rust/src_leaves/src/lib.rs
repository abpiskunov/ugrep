//! Rust side of the `src_leaves` component (src/glob.cpp, later zopen and zstream).
//! One bridge module (`ffi`); all boundary code is here. The bodies are stubs until the Port tasks.

mod signatures;

#[cxx::bridge(namespace = "ugrep_rust")]
mod ffi {
    extern "Rust" {
        /// Seam function `src_leaves.glob_match`. Arguments are the bytes of the C strings, without NUL.
        fn glob_match(
            pathname: &[u8],
            basename: &[u8],
            glob: &[u8],
            icase: bool,
            lead: bool,
            path: bool,
        ) -> bool;
    }
}

fn glob_match(pathname: &[u8], basename: &[u8], glob: &[u8], icase: bool, lead: bool, path: bool) -> bool {
    signatures::glob_match(pathname, basename, glob, icase, lead, path)
}
