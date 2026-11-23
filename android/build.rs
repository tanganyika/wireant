// Android can't run an extracted executable (W^X since Android 10), so this build has no
// NEC-2 process engine: nec.rs sees an empty binary and reports NEC-2 as unavailable.
fn main() {
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("nec2c");
    std::fs::write(out, b"").unwrap();
}
