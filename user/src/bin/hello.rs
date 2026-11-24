//! Prints a greeting and its arguments.

#![no_std]
#![no_main]

dante::main!(main);

fn main(args: &str) {
    if args.is_empty() {
        dante::println!("Hello, world");
    } else {
        dante::println!("Hello, {args}");
    }
}
