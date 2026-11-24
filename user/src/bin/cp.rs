//! Copies a file: cp <from> <to>. If <to> is a folder, the copy goes inside it.

#![no_std]
#![no_main]

use dante::format;

dante::main!(main);

fn main(args: &str) {
    let mut parts = args.split_whitespace();
    let (Some(from), Some(to), None) = (parts.next(), parts.next(), parts.next()) else {
        dante::println!("usage: cp <from> <to>");
        dante::exit(2);
    };
    let data = match dante::fs::read(from) {
        Ok(d) => d,
        Err(e) => {
            dante::println!("cp: {from}: {e}");
            dante::exit(1);
        }
    };
    let name = from.rsplit('/').next().unwrap_or(from);
    let target = if dante::fs::list(to).is_ok() { format!("{}/{name}", to.trim_end_matches('/')) } else { to.into() };
    if let Err(e) = dante::fs::write(&target, &data) {
        dante::println!("cp: {target}: {e}");
        dante::exit(1);
    }
}
