use cmake::Config;

fn main() {
    let dst = Config::new(".")
        .define("CMAKE_C_FLAGS", "-ffunction-sections -fdata-sections -fPIC -w")
        .define("CMAKE_CXX_FLAGS", "-ffunction-sections -fdata-sections -fPIC -w")
        .build_target("viogsf")
        .build();

    println!("cargo:rustc-link-search=native={}/build", dst.display());
    println!("cargo:rustc-link-lib=static=viogsf");

    // C++ standard library
    #[cfg(target_os = "linux")]
    println!("cargo:rustc-link-lib=stdc++");
    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-lib=c++");
}
