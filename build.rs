fn main() {
    if std::env::var("CARGO_FEATURE_HOLD").is_ok() {
        tonic_prost_build::configure()
            .protoc_arg("--experimental_allow_proto3_optional")
            .compile_protos(&["protos/hold.proto"], &["protos"])
            .unwrap_or_else(|e| panic!("Could not build protos: {e}"));
    }
}
