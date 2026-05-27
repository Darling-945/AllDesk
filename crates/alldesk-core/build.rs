fn main() {
    let proto_file = "../../proto/message.proto";
    if std::path::Path::new(proto_file).exists() {
        prost_build::Config::new()
            .compile_protos(&[proto_file], &["../../proto/"])
            .expect("Failed to compile protobuf");
    }
}
