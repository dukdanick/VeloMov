//! Windows builds: embed the icon, version information and manifest into velomov.exe.
//! Needs a resource compiler (llvm-windres / x86_64-w64-mingw32-windres, or rc.exe with
//! MSVC); without one the program still builds, just without an icon.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=packaging/windows/velomov.rc");
    println!("cargo:rerun-if-changed=packaging/windows/velomov.ico");
    println!("cargo:rerun-if-changed=packaging/windows/velomov.exe.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let ver = std::env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts = ver.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let (a, b, c) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    std::fs::write(
        out.join("version.h"),
        format!("#define VER_MAJOR {a}\n#define VER_MINOR {b}\n#define VER_PATCH {c}\n#define VER_STRING \"{ver}\"\n"),
    )
    .unwrap();
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("packaging").join("windows");
    let rc = dir.join("velomov.rc");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let res = out.join("velomov.res");
        let ok = Command::new("rc.exe")
            .args(["/nologo", "/I"])
            .arg(&out)
            .arg("/I")
            .arg(&dir)
            .arg("/fo")
            .arg(&res)
            .arg(&rc)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            println!("cargo:rustc-link-arg-bins={}", res.display());
        } else {
            println!("cargo:warning=rc.exe not found: velomov.exe gets no icon");
        }
        return;
    }
    let obj = out.join("velomov-res.o");
    let tools = [std::env::var("WINDRES").unwrap_or_default(), "x86_64-w64-mingw32-windres".into(), "llvm-windres".into(), "windres".into()];
    for tool in tools.iter().filter(|t| !t.is_empty()) {
        let ok = Command::new(tool)
            .arg("-I")
            .arg(&out)
            .arg("-I")
            .arg(&dir)
            .args(["-O", "coff", "-i"])
            .arg(&rc)
            .arg("-o")
            .arg(&obj)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            println!("cargo:rustc-link-arg-bins={}", obj.display());
            return;
        }
    }
    println!("cargo:warning=no windres found: velomov.exe gets no icon");
}
