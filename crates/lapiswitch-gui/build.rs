use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/branding/lapiswitch.ico");
    println!("cargo:rerun-if-changed=resources/windows.rc");
    println!("cargo:rerun-if-env-changed=WINDRES");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("build output directory"));
    fs::copy(
        root.join("assets/branding/lapiswitch.ico"),
        out.join("lapiSwitch.ico"),
    )
    .expect("copy application icon");
    let version_num = ["MAJOR", "MINOR", "PATCH"]
        .map(|part| env::var(format!("CARGO_PKG_VERSION_{part}")).expect("package version"))
        .join(",")
        + ",0";
    let resource = fs::read_to_string(root.join("resources/windows.rc"))
        .expect("Windows resource template")
        .replace("@VERSION_NUM@", &version_num)
        .replace(
            "@VERSION_STRING@",
            &env::var("CARGO_PKG_VERSION").expect("package version"),
        );
    fs::write(out.join("lapiSwitch.rc"), resource).expect("write Windows resource source");

    let msvc = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let compiled = out.join(if msvc {
        "lapiSwitch.res"
    } else {
        "lapiSwitch.o"
    });
    let mut compiler = if msvc {
        let host = env::var("HOST").expect("build host");
        let sdk =
            find_msvc_tools::find_windows_sdk(host.split('-').next().expect("host architecture"))
                .expect("Windows SDK is required to embed the application icon");
        let rc = sdk
            .path()
            .map(|dir| dir.join("rc.exe"))
            .find(|path| path.is_file())
            .expect("Windows SDK rc.exe was not found");
        let mut command = Command::new(rc);
        command
            .arg("/nologo")
            .arg("/fo")
            .arg(&compiled)
            .arg("lapiSwitch.rc");
        command
    } else {
        let mut command = Command::new(env::var_os("WINDRES").unwrap_or_else(|| "windres".into()));
        command
            .arg("lapiSwitch.rc")
            .arg("-O")
            .arg("coff")
            .arg("-o")
            .arg(&compiled);
        command
    };
    let status = compiler
        .current_dir(&out)
        .status()
        .expect("run Windows resource compiler");
    assert!(
        status.success(),
        "Windows resource compilation failed: {status}"
    );
    println!("cargo:rustc-link-arg-bin=lapiSwitch={}", compiled.display());
}
