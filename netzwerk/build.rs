use bindgen::builder;

fn main() {
    let bindings = builder().header("ffi/pfvar.h")
        .generate()
        .expect("failed to generate PF bindings");

    bindings.write_to_file("src/pf/bindings.rs")
        .expect("failed to write PF bindings on disk");
}
