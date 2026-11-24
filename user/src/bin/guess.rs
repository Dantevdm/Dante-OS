//! Guess the number between 1 and 100.

#![no_std]
#![no_main]

dante::main!(main);

fn main(_args: &str) {
    // The time since start is a fine source of randomness for a game.
    let mut x = dante::millis().wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    x ^= x >> 33;
    let target = x % 100 + 1;
    dante::println!("Guess a number from 1 to 100.");
    let mut tries = 0;
    loop {
        dante::print!("> ");
        let Some(line) = dante::io::read_line() else { return };
        let Ok(n) = line.trim().parse::<u64>() else {
            dante::println!("Type a number.");
            continue;
        };
        tries += 1;
        match n.cmp(&target) {
            core::cmp::Ordering::Less => dante::println!("Higher."),
            core::cmp::Ordering::Greater => dante::println!("Lower."),
            core::cmp::Ordering::Equal => {
                dante::println!("{target}, in {tries} tries.");
                return;
            }
        }
    }
}
