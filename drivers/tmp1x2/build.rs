fn main() {
    if std::env::var_os("CARGO_FEATURE_EMBASSY_EXAMPLE").is_none()
        && std::env::var_os("CARGO_FEATURE_LINK_CHECK").is_none()
    {
        return;
    }
    let rp2040 = std::env::var_os("CARGO_FEATURE_RP2040").is_some()
        || std::env::var_os("CARGO_FEATURE_EXAMPLE_RP2040").is_some();
    let rp235xa = std::env::var_os("CARGO_FEATURE_RP235XA").is_some()
        || std::env::var_os("CARGO_FEATURE_EXAMPLE_RP235XA").is_some();
    assert!(rp2040 ^ rp235xa, "select exactly one MCU feature");
    let (target, memory) = if rp2040 {
        ("thumbv6m-none-eabi", "examples/memory-rp2040.x")
    } else {
        ("thumbv8m.main-none-eabihf", "examples/memory-rp2350.x")
    };
    assert_eq!(std::env::var("TARGET").unwrap(), target);
    println!("cargo:rerun-if-changed={memory}");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::copy(memory, out.join("memory.x")).unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    for arg in ["--nmagic", "-Tlink.x", "-Tdefmt.x"] {
        println!("cargo:rustc-link-arg-examples={arg}");
    }
    if rp2040 {
        println!("cargo:rustc-link-arg-examples=-Tlink-rp.x");
    }
}
