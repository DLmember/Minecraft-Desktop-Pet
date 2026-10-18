fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=../assets/icon.ico");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app-res.o");
    let status = std::process::Command::new("windres")
        .args(["--codepage=65001", "-i", "app.rc", "-O", "coff", "-o"])
        .arg(&out)
        .status()
        .expect("windres required");
    assert!(status.success());
    println!("cargo:rustc-link-arg={}", out.display());
}
