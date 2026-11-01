use std::path::Path;

fn main() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
    // rust-embed needs the folder at compile time; on clean checkouts create a
    // placeholder so `cargo build` works before `npm run build`.
    if !dist.join("index.html").exists() {
        let _ = std::fs::create_dir_all(&dist);
        let _ = std::fs::write(dist.join("index.html"), b"<!--HALO-PLACEHOLDER-->\n");
    }
    println!("cargo:rerun-if-changed=../../web/dist");
}
