use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=app.ico");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let res_obj = out_dir.join("app_res.o");

    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "gnu" {
        let mut cmd = Command::new("windres");
        cmd.args(["app.rc", "-O", "coff", "-o"]).arg(&res_obj);

        // Provide w64devkit/bin only to windres subprocess so it finds gcc preprocessor
        if let Ok(userprofile) = env::var("USERPROFILE") {
            let mingw_bin = PathBuf::from(userprofile).join("w64devkit").join("bin");
            if mingw_bin.exists() {
                let current_path = env::var("PATH").unwrap_or_default();
                cmd.env("PATH", format!("{};{}", mingw_bin.display(), current_path));
            }
        }

        match cmd.status() {
            Ok(status) if status.success() && res_obj.exists() => {
                println!("cargo:rustc-link-arg={}", res_obj.display());
            }
            Ok(status) => {
                println!("cargo:warning=windres exited with status: {}", status);
            }
            Err(e) => {
                println!("cargo:warning=failed to run windres: {}", e);
            }
        }
    }
}
