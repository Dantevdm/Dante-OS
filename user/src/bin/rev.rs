//! Prints each typed line backwards. Ctrl+D ends.

#![no_std]
#![no_main]

use dante::String;

dante::main!(main);

fn main(_args: &str) {
    while let Some(line) = dante::io::read_line() {
        let back: String = line.chars().rev().collect();
        dante::println!("{back}");
    }
}
