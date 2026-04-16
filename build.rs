fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_root = "../medusa-protos/src/main/proto";

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(
            &[
                format!("{proto_root}/farisland/threed/v1/point_cloud_types.proto"),
                format!("{proto_root}/farisland/threed/v1/point_cloud.proto"),
                format!("{proto_root}/farisland/threed/v1/mesh.proto"),
                format!("{proto_root}/farisland/threed/v1/threed_scan.proto"),
                format!("{proto_root}/farisland/threed/v1/metrology_callback.proto"),
                format!("{proto_root}/farisland/module/v1/module_registry.proto"),
                format!("{proto_root}/farisland/common/v1/identifiers.proto"),
            ],
            &[proto_root],
        )?;

    Ok(())
}
