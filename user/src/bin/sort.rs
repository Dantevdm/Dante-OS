//! Prints the lines of a file (or typed input) in order.

#![no_std]
#![no_main]

use dante::{String, Vec};

dante::main!(main);

fn main(args: &str) {
    let path = args.trim();
    let text = if path.is_empty() {
        String::from_utf8_lossy(&dante::io::read_all()).into_owned()
    } else {
        match dante::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                dante::println!("sort: {path}: {e}");
                dante::exit(1);
            }
        }
    };
    let mut lines: Vec<&str> = text.lines().collect();
    lines.sort_unstable();
    for l in lines {
        dante::println!("{l}");
    }
}
