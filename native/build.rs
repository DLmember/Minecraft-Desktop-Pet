fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-env-changed=CXX");
    println!("cargo:rerun-if-env-changed=WINDRES");
    println!("cargo:rerun-if-changed=../assets/github.ico");
    println!("cargo:rerun-if-changed=../assets/github-dark.ico");
    println!("cargo:rerun-if-changed=audio_probe.cpp");
    let audio=std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("audio-probe.o");
    let status=std::process::Command::new(std::env::var("CXX").unwrap_or_else(|_| "clang++".into())).args(["-c","-O2","-fno-exceptions","-fno-rtti","audio_probe.cpp","-o"]).arg(&audio).status().expect("clang++ required");
    assert!(status.success());
    println!("cargo:rustc-link-arg={}",audio.display());
    println!("cargo:rustc-link-lib=ole32");
    println!("cargo:rustc-link-lib=uuid");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=../assets/icon.ico");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app-res.o");
    let status = std::process::Command::new(std::env::var("WINDRES").unwrap_or_else(|_| "windres".into()))
        .args(["--codepage=65001", "-i", "app.rc", "-O", "coff", "-o"])
        .arg(&out)
        .status()
        .expect("windres required");
    assert!(status.success());
    println!("cargo:rustc-link-arg={}", out.display());
}
