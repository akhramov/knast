use bindgen::builder;

fn main() {
    let bindings = builder()
        .header("ffi/ffi.h")
        .generate()
        .expect("failed to generate bindings");

    bindings
        .write_to_file("src/bindings.rs")
        .expect("failed to write bindings on disk");
}
