// SPDX-License-Identifier: MIT
fn main() {
    println!(
        "cargo:rustc-env=AISH_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
}
