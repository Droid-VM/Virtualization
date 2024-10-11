fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_file = "../DebianService/proto/DebianService.proto";

    tonic_build::compile_protos(proto_file).unwrap();

    Ok(())
}
