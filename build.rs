fn main() {
    println!(
        "cargo:rustc-env=RIME_SHARED_DATA_DIR={}",
        std::env::var("RIME_SHARED_DATA_DIR").unwrap()
    );

    let lib_definitions = [("rime", "#include <rime_api.h>")];
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    for (lib_name, header_content) in &lib_definitions {
        let meta_data = pkg_config::probe_library(lib_name).unwrap();
        let mut builder = bindgen::Builder::default()
            .header_contents(&format!("wrapper_{}.h", lib_name), header_content);
        for path in &meta_data.include_paths {
            builder = builder.clang_arg(format!("-I{}", path.display()));
        }
        let bindings = builder.generate().unwrap();
        let output_file_path = out_dir.join(format!("{}_bindings.rs", lib_name));
        bindings.write_to_file(output_file_path).unwrap();
    }
}
