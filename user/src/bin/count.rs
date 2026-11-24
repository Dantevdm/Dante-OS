//! Counts up to a number (10 by default), one step every half second.

#![no_std]
#![no_main]

dante::main!(main);

fn main(args: &str) {
    let n: u32 = args.trim().parse().unwrap_or(10);
    for i in 1..=n {
        dante::println!("{i}");
        if i < n {
            dante::sleep(500);
        }
    }
}
