// The web UI is embedded from `web/dist`. A Rust-only build (CI, tests) has no built UI yet,
// so an empty folder keeps `rust-embed` happy; the server then answers with a short notice.
fn main() {
    let dist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
    if !dist.exists() {
        std::fs::create_dir_all(&dist).expect("create web/dist");
    }
    println!("cargo:rerun-if-changed=../../web/dist");
    println!("cargo:rerun-if-changed=../../install/install.sh");
}
