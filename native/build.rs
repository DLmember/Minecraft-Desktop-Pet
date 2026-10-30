fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=app.manifest");
    // 清单里的处理器架构必须与目标一致，否则 x86/arm64 上整份清单会被忽略
    // （DPI 感知、Common Controls v6 都会失效）。
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86") => "x86",
        Ok("aarch64") => "arm64",
        _ => "amd64",
    };
    let manifest = std::fs::read_to_string("app.manifest").expect("app.manifest");
    let manifest = manifest.replace(
        r#"processorArchitecture="amd64""#,
        &format!(r#"processorArchitecture="{arch}""#),
    );
    let manifest_path = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app.manifest");
    std::fs::write(&manifest_path, manifest).expect("write app.manifest");
    let rc = std::fs::read_to_string("app.rc").expect("app.rc");
    let rc = rc.replace("1 24 \"app.manifest\"", &format!("1 24 \"{}\"", manifest_path.display()));
    let rc_path = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app.rc");
    std::fs::write(&rc_path, rc).expect("write app.rc");
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
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=../assets/icon.ico");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app-res.o");
    let status = std::process::Command::new(std::env::var("WINDRES").unwrap_or_else(|_| "windres".into()))
        .args(["--codepage=65001", "-i"])
        .arg(&rc_path)
        .args(["-O", "coff", "-o"])
        .arg(&out)
        .status()
        .expect("windres required");
    assert!(status.success());
    println!("cargo:rustc-link-arg={}", out.display());
}
