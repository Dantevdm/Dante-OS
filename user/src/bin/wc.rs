//! Counts lines, words and bytes in files, or in typed input if none are given.

#![no_std]
#![no_main]

dante::main!(main);

fn count(data: &[u8]) -> (usize, usize, usize) {
    let text = core::str::from_utf8(data).unwrap_or("");
    let lines = data.iter().filter(|&&b| b == b'\n').count();
    (lines, text.split_whitespace().count(), data.len())
}

fn main(args: &str) {
    if args.trim().is_empty() {
        let (l, w, b) = count(&dante::io::read_all());
        dante::println!("{l:>7} {w:>7} {b:>7}");
        return;
    }
    let mut total = (0, 0, 0);
    let mut files = 0;
    for path in args.split_whitespace() {
        match dante::fs::read(path) {
            Ok(data) => {
                let (l, w, b) = count(&data);
                dante::println!("{l:>7} {w:>7} {b:>7} {path}");
                total = (total.0 + l, total.1 + w, total.2 + b);
                files += 1;
            }
            Err(e) => dante::println!("wc: {path}: {e}"),
        }
    }
    if files > 1 {
        dante::println!("{:>7} {:>7} {:>7} total", total.0, total.1, total.2);
    }
}
