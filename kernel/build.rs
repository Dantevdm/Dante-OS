//! Builds the user programs in ../user and lists them for the kernel, which
//! copies them into /Programs on the disk at startup.

use std::env;
use std::fmt::Write;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let user = manifest.parent().unwrap().join("user");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    for p in ["src", "Cargo.toml", "link.ld", ".cargo/config.toml"] {
        println!("cargo:rerun-if-changed={}", user.join(p).display());
    }

    // Run a clean cargo for the user crate: drop what this build passes down.
    let mut cargo = Command::new("cargo");
    cargo.current_dir(&user).args(["build", "--release", "--offline"]);
    for (k, _) in env::vars() {
        let inherited = (k.starts_with("CARGO") && k != "CARGO_HOME")
            || k.starts_with("RUSTC")
            || matches!(k.as_str(), "RUSTFLAGS" | "RUSTDOC" | "RUSTUP_TOOLCHAIN" | "TARGET" | "HOST" | "OUT_DIR" | "PROFILE" | "OPT_LEVEL" | "DEBUG" | "NUM_JOBS");
        if inherited {
            cargo.env_remove(k);
        }
    }
    let status = cargo.status().expect("could not run cargo for the user programs");
    assert!(status.success(), "the user programs failed to build");

    let built = user.join("target/x86_64-unknown-none/release");
    let mut names: Vec<String> = fs::read_dir(user.join("src/bin"))
        .unwrap()
        .filter_map(|e| e.ok()?.path().file_stem()?.to_str().map(String::from))
        .collect();
    names.sort();
    let mut table = String::from("pub const PROGRAMS: &[(&str, &[u8])] = &[\n");
    for name in &names {
        let dest = out.join(name);
        fs::copy(built.join(name), &dest).expect("missing user program");
        writeln!(table, "    ({name:?}, include_bytes!({:?})),", dest.display().to_string()).unwrap();
    }
    table.push_str("];\n");
    fs::write(out.join("programs.rs"), table).unwrap();
}
