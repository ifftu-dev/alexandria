use std::path::PathBuf;
use std::process::Command;

const W2C2_REVISION: &str = "9de3c2be5a4ed8ef5fdbd536e445120594fb8530";

fn run(command: &mut Command) {
    let status = command
        .env_remove("CMAKE_TOOLCHAIN_FILE")
        .env_remove("IPHONEOS_DEPLOYMENT_TARGET")
        .env_remove("SDKROOT")
        .env_remove("CC")
        .env_remove("CXX")
        .status()
        .expect("run pinned witness transpiler build");
    assert!(status.success(), "witness transpiler build failed");
}

fn main() {
    println!("cargo:rerun-if-env-changed=PERSONHOOD_CIRCUIT_DIR");
    let circuits = std::env::var("PERSONHOOD_CIRCUIT_DIR")
        .expect("set PERSONHOOD_CIRCUIT_DIR using prepare-personhood-lab.py");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let transpiler = out.join("w2c2");
    if !transpiler.join(".git").exists() {
        run(Command::new("git")
            .args([
                "clone",
                "--no-checkout",
                "https://github.com/vivianjeng/w2c2",
            ])
            .arg(&transpiler));
    }
    run(Command::new("git").arg("-C").arg(&transpiler).args([
        "checkout",
        "--detach",
        W2C2_REVISION,
    ]));
    let build = transpiler.join("build");
    let revision_stamp = build.join(".personhood-built-revision");
    if std::fs::read_to_string(&revision_stamp).ok().as_deref() != Some(W2C2_REVISION)
        || !build.join("w2c2/w2c2").exists()
    {
        if build.exists() {
            std::fs::remove_dir_all(&build).expect("discard stale transpiler build");
        }
        run(Command::new("git").arg("-C").arg(&transpiler).args([
            "submodule",
            "update",
            "--init",
            "--recursive",
        ]));
        run(Command::new("cmake")
            .arg("-S")
            .arg(&transpiler)
            .arg("-B")
            .arg(&build));
        run(Command::new("cmake")
            .arg("--build")
            .arg(&build)
            .args(["--parallel", "6"]));
        std::fs::write(revision_stamp, W2C2_REVISION).expect("record transpiler revision");
    }
    // Avoid an unrelated w2c2 from PATH: rust-witness finds this pinned binary.
    let path = std::env::var_os("PATH").expect("PATH");
    let paths = std::iter::once(transpiler.join("build/w2c2")).chain(std::env::split_paths(&path));
    std::env::set_var(
        "PATH",
        std::env::join_paths(paths).expect("transpiler PATH"),
    );
    let source = PathBuf::from(circuits).join("aadhaar-verifier.wasm");
    println!("cargo:rerun-if-changed={}", source.display());
    let selected = out.join("selected-circuit");
    std::fs::create_dir_all(&selected).expect("selected circuit directory");
    let target = selected.join("aadhaar-verifier.wasm");
    if std::fs::read(&target).ok() != Some(std::fs::read(&source).expect("published WASM")) {
        std::fs::copy(source, target).expect("copy published circuit");
    }
    rust_witness::transpile::transpile_wasm(selected.to_string_lossy().into_owned());
}
