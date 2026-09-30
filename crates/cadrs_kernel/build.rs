fn main() {
    // OCCT's OSD layer (built statically by the `occt` feature) calls advapi32 (security
    // descriptors, registry, GetUserNameW), which opencascade-sys doesn't link on Windows.
    let occt = std::env::var_os("CARGO_FEATURE_OCCT").is_some();
    let windows = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    if occt && windows {
        println!("cargo:rustc-link-lib=dylib=advapi32");
    }
}
