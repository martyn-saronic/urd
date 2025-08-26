fn main() {
    // Pure Rust implementation - no C++ dependencies needed
    println!("cargo:rerun-if-changed=src/rtde.rs");
}