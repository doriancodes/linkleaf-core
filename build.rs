fn main() {
    let mut config = prost_build::Config::new();
    // Generate into OUT_DIR (default). We'll `include!` it from src/main.rs
    config
        .compile_protos(&["proto/linkleaf/v1/feed.proto"], &["proto"])
        .expect("failed to compile protos");

    // Re-run build if the .proto changes
    println!("cargo:rerun-if-changed=proto/linkleaf/v1/feed.proto");
}
