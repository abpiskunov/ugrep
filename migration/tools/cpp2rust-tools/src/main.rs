fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(cpp2rust_tools::run(&argv));
}
