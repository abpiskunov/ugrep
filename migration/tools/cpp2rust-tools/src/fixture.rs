//! `__fixture`: small portable programs the tests run as child processes (instead of shell or
//! Python one-liners), so the tests run the same on Linux, macOS and Windows. Not for users.

use std::io::Read;

pub fn main(argv: &[String]) -> i32 {
    match argv.first().map(String::as_str) {
        // like a tiny CLI: prints its arguments and LC_ALL, writes stdin upper-cased to out.txt
        Some("echo") => {
            let args = &argv[1..];
            println!("hi {:?} {:?}", args, std::env::var("LC_ALL").ok());
            let mut input = String::new();
            let _ = std::io::stdin().read_to_string(&mut input);
            std::fs::write("out.txt", input.to_uppercase()).unwrap();
            if args.iter().any(|a| a == "--fail") { 3 } else { 0 }
        }
        // not deterministic
        Some("random") => {
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
            println!("{:x}{:x}", t.subsec_nanos(), std::process::id());
            0
        }
        // write <path> <text>
        Some("write") if argv.len() == 3 => {
            std::fs::write(&argv[1], &argv[2]).unwrap();
            0
        }
        // junit <path> <name>... : passing test cases
        Some("junit") if argv.len() >= 2 => {
            let cases: String = argv[2..]
                .iter()
                .map(|n| format!("<testcase classname=\"a\" name=\"{n}\"/>"))
                .collect();
            std::fs::write(&argv[1], format!("<testsuite>{cases}</testsuite>")).unwrap();
            0
        }
        // seam <path> <divergent>
        Some("seam") if argv.len() == 3 => {
            std::fs::write(&argv[1], format!("{{\"divergent_cases\": {}, \"s\": [{{\"n\": 7}}]}}", argv[2])).unwrap();
            0
        }
        Some("exit") if argv.len() == 2 => argv[1].parse().unwrap_or(1),
        // sleep <seconds> [<path>]: writes <path> after sleeping (shows a process outlived a kill)
        Some("sleep") if argv.len() == 2 || argv.len() == 3 => {
            std::thread::sleep(std::time::Duration::from_secs_f64(argv[1].parse().unwrap_or(1.0)));
            if let Some(p) = argv.get(2) {
                std::fs::write(p, "late").unwrap();
            }
            0
        }
        // the working directory with `/` separators, as portable programs print paths
        Some("pwd") => {
            println!("{}", std::env::current_dir().unwrap().to_string_lossy().replace('\\', "/"));
            0
        }
        // env NAME...: NAME=value lines
        Some("env") => {
            for k in &argv[1..] {
                println!("{k}={}", std::env::var(k).unwrap_or_default());
            }
            0
        }
        _ => {
            eprintln!("__fixture: echo | random | write P T | junit P N... | seam P D | exit N | sleep S [P] | pwd | env N...");
            2
        }
    }
}
