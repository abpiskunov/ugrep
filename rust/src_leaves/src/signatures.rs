//! Types and signatures of the port units of `src_leaves`, fixed by the Prepare seam task.
//! Bodies are stubs; the Port task for the unit replaces them without changing a signature.

/// C++ `bool glob_match(const char *pathname, const char *basename, const char *glob,
/// bool icase, bool lead, bool path)` from src/glob.hpp. Text arguments are bytes.
pub fn glob_match(_pathname: &[u8], _basename: &[u8], _glob: &[u8], _icase: bool, _lead: bool, _path: bool) -> bool {
    todo!("Port: src_leaves.glob_match")
}
