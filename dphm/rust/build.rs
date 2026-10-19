// Generated/maintained with AI; see repository disclaimers.md.
fn main() {
    println!("cargo:rerun-if-env-changed=DPHM_CUDA_LIB_DIR");
    if std::env::var_os("CARGO_FEATURE_CUDA").is_some() {
        let path = std::env::var("DPHM_CUDA_LIB_DIR")
            .expect("build cuda_solver first and set DPHM_CUDA_LIB_DIR to its build directory");
        println!("cargo:rustc-link-search=native={path}");
        println!("cargo:rustc-link-lib=dphm_cuda_native");
        println!("cargo:rustc-link-arg=-Wl,-rpath,{path}");
    }
}
