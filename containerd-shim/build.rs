use ttrpc_codegen::Codegen;

fn main() {
    let shim_inputs = vec![
        "proto/shim.proto",
        "proto/google/protobuf/any.proto",
        "proto/google/protobuf/empty.proto",
        "proto/google/protobuf/timestamp.proto",
        "proto/github.com/containerd/containerd/api/types/mount.proto",
        "proto/github.com/containerd/containerd/api/types/task/task.proto",
    ];
    Codegen::new()
        .out_dir("src/protocols")
        .inputs(&shim_inputs)
        .include("proto")
        .rust_protobuf()
        .run()
        .expect("Failed to generate ttrpc server code");
}
