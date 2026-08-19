use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../../tools/netvan-hwmon/Program.cs");
    println!("cargo:rerun-if-changed=../../tools/netvan-hwmon/NetvanHwmon.csproj");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let proj = manifest_dir.join("../../tools/netvan-hwmon/NetvanHwmon.csproj");
    if !proj.is_file() {
        println!("cargo:warning=netvan-hwmon project missing; thermal helper will not be built");
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let Some(profile_dir) = out_dir.ancestors().nth(3) else {
        println!("cargo:warning=could not resolve cargo profile dir for netvan-hwmon");
        return;
    };

    let status = Command::new("dotnet")
        .args([
            "publish",
            proj.to_str().unwrap_or_default(),
            "-c",
            "Release",
            "-r",
            "win-x64",
            "--self-contained",
            "false",
            "-o",
            profile_dir.to_str().unwrap_or_default(),
        ])
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => println!("cargo:warning=dotnet publish netvan-hwmon exited {s}"),
        Err(e) => println!("cargo:warning=dotnet not available ({e}); thermal helper not published"),
    }
}
