fn main() {
    println!("cargo:rerun-if-changed=../../cpp/limiter/src/aperture_limiter.cpp");
    println!("cargo:rerun-if-changed=../../cpp/limiter/include/aperture_limiter.h");
    cc::Build::new()
        .cpp(true)
        .file("../../cpp/limiter/src/aperture_limiter.cpp")
        .include("../../cpp/limiter/include")
        .flag_if_supported("-std=c++17")
        .flag_if_supported("-fPIC")
        .compile("aperture_limiter");
}
