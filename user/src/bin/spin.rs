//! Keeps the processor busy and never sleeps. The desktop stays responsive
//! because the kernel takes turns between programs. Stop it with Ctrl+C.

#![no_std]
#![no_main]

dante::main!(main);

fn main(_args: &str) {
    let start = dante::millis();
    let mut next = start + 1000;
    let mut loops: u64 = 0;
    loop {
        loops += 1;
        let now = dante::millis();
        if now >= next {
            dante::println!("{} s, {} loops", (now - start) / 1000, loops);
            next += 1000;
        }
    }
}
