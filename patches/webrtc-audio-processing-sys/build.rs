use anyhow::Result;
use regex::Regex;
use std::{
    env,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const DEPLOYMENT_TARGET_VAR: &str = "MACOSX_DEPLOYMENT_TARGET";

fn out_dir() -> PathBuf {
    std::env::var("OUT_DIR")
        .expect("OUT_DIR environment var not set.")
        .into()
}

#[cfg(not(feature = "bundled"))]
mod webrtc {
    use super::*;
    use anyhow::bail;

    const LIB_NAME: &str = "webrtc-audio-processing";

    pub(super) fn get_build_paths() -> Result<(PathBuf, PathBuf)> {
        let (pkgconfig_include_path, pkgconfig_lib_path) = find_pkgconfig_paths()?;

        let include_path = std::env::var("WEBRTC_AUDIO_PROCESSING_INCLUDE")
            .ok()
            .map(|x| x.into())
            .or(pkgconfig_include_path);
        let lib_path = std::env::var("WEBRTC_AUDIO_PROCESSING_LIB")
            .ok()
            .map(|x| x.into())
            .or(pkgconfig_lib_path);

        println!("{:?}, {:?}", include_path, lib_path);

        match (include_path, lib_path) {
            (Some(include_path), Some(lib_path)) => Ok((include_path, lib_path)),
            _ => {
                eprintln!("Couldn't find either header or lib files for {}.", LIB_NAME);
                eprintln!("See the crate README for installation instructions, or use the 'bundled' feature to statically compile.");
                bail!("Aborting compilation due to linker failure.");
            }
        }
    }

    pub(super) fn build_if_necessary() -> Result<()> {
        Ok(())
    }

    fn find_pkgconfig_paths() -> Result<(Option<PathBuf>, Option<PathBuf>)> {
        Ok(pkg_config::Config::new()
            .probe(LIB_NAME)
            .and_then(|mut lib| Ok((lib.include_paths.pop(), lib.link_paths.pop())))?)
    }
}

#[cfg(feature = "bundled")]
mod webrtc {
    use super::*;
    use anyhow::{anyhow, bail};

    const BUNDLED_SOURCE_PATH: &str = "./webrtc-audio-processing";

    pub(super) fn get_build_paths() -> Result<(PathBuf, PathBuf)> {
        let include_path = out_dir().join(BUNDLED_SOURCE_PATH);
        let lib_path = out_dir().join("lib");
        Ok((include_path, lib_path))
    }

    fn copy_source_to_out_dir() -> Result<PathBuf> {
        use fs_extra::dir::CopyOptions;

        if Path::new(BUNDLED_SOURCE_PATH).read_dir()?.next().is_none() {
            eprintln!("The webrtc-audio-processing source directory is empty.");
            eprintln!("See the crate README for installation instructions.");
            eprintln!("Remember to clone the repo recursively if building from source.");
            bail!("Aborting compilation because bundled source directory is empty.");
        }

        let out_dir = out_dir();
        let mut options = CopyOptions::new();
        options.overwrite = true;

        fs_extra::dir::copy(BUNDLED_SOURCE_PATH, &out_dir, &options)?;

        Ok(out_dir.join(BUNDLED_SOURCE_PATH))
    }

    pub(super) fn build_if_necessary() -> Result<()> {
        let build_dir = copy_source_to_out_dir()?;

        if cfg!(target_os = "macos") {
            run_command(&build_dir, "glibtoolize", None)?;
        } else {
            run_command(&build_dir, "libtoolize", None)?;
        }

        run_command(&build_dir, "aclocal", None)?;
        run_command(&build_dir, "automake", Some(&["--add-missing", "--copy"]))?;
        run_command(&build_dir, "autoconf", None)?;

        let target = std::env::var("TARGET").unwrap();
        if cfg!(windows) {
            let prefix = windows_shell_path(&out_dir())?;
            let srcdir = windows_shell_path(&build_dir)?;
            let host_arg = format!("--host={target}");
            let prefix_arg = format!("--prefix={prefix}");
            let srcdir_arg = format!("--srcdir={srcdir}");
            run_command(
                &build_dir,
                "./configure",
                Some(&[
                    prefix_arg.as_str(),
                    srcdir_arg.as_str(),
                    "--disable-shared",
                    "--enable-static",
                    host_arg.as_str(),
                ]),
            )?;
            run_command(&build_dir, "make", None)?;
            run_command(&build_dir, "make", Some(&["install"]))?;
        } else {
            autotools::Config::new(build_dir)
                .cflag("-fPIC")
                .cxxflag("-fPIC")
                .config_option("host", Some(&target))
                .disable_shared()
                .enable_static()
                .build();
        }

        Ok(())
    }

    fn windows_shell_path(path: &Path) -> Result<String> {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let raw = path
            .to_str()
            .ok_or_else(|| anyhow!("non-utf8 path: {path:?}"))?
            .replace('\\', "/");

        if raw.len() >= 3 && raw.as_bytes()[1] == b':' {
            let drive = raw[..1].to_ascii_lowercase();
            Ok(format!("/{}/{}", drive, raw[2..].trim_start_matches('/')))
        } else {
            Ok(raw)
        }
    }

    fn windows_tool_dirs() -> Vec<PathBuf> {
        let dirs = Vec::new();

        #[cfg(windows)]
        {
            let mut dirs = dirs;
            if let Some(dir) = std::env::var_os("VCToolsInstallDir")
                .map(PathBuf::from)
                .map(|root| root.join("bin").join("Hostx64").join("x64"))
                .filter(|dir| dir.is_dir())
            {
                dirs.push(dir);
            }

            for sdk_var in ["WindowsSdkVerBinPath", "WindowsSdkBinPath"] {
                if let Some(root) = std::env::var_os(sdk_var).map(PathBuf::from) {
                    let x64_dir = root.join("x64");
                    if x64_dir.is_dir() {
                        dirs.push(x64_dir);
                    } else if root.is_dir() {
                        dirs.push(root);
                    }
                }
            }

            return dirs;
        }

        dirs
    }

    fn windows_augmented_path() -> Result<String> {
        let mut entries = windows_tool_dirs();
        if let Some(existing_path) = std::env::var_os("PATH") {
            entries.extend(std::env::split_paths(&existing_path));
        }

        let mut deduped = Vec::new();
        for entry in entries {
            if entry.as_os_str().is_empty() || deduped.iter().any(|existing| existing == &entry) {
                continue;
            }
            deduped.push(entry);
        }

        let joined = std::env::join_paths(deduped)
            .map_err(|e| anyhow!("failed to join Windows tool PATH entries: {e}"))?;
        Ok(joined.to_string_lossy().into_owned())
    }

    fn windows_tool(var: &str, default: &str) -> String {
        let scoped_var = format!("WEBRTC_{var}");
        std::env::var(&scoped_var)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| default.to_string())
    }

    fn run_command<P: AsRef<Path>>(
        curr_dir: P,
        cmd: &str,
        args_opt: Option<&[&str]>,
    ) -> Result<()> {
        let curr_dir = curr_dir.as_ref();
        let mut command = if cfg!(windows) {
            // On Windows the autotools helpers and ./configure entrypoint run under MSYS2 bash.
            // Explicitly `cd` into the translated source/build directory because relying on the
            // Win32 current directory can leave bash in the wrong place for aclocal/configure.
            let shell_dir = windows_shell_path(curr_dir)?;
            let bash = std::env::var_os("MSYS2_BASH_EXE")
                .map(PathBuf::from)
                .filter(|path| path.exists())
                .unwrap_or_else(|| PathBuf::from(r"C:\msys64\usr\bin\bash.exe"));
            let mut command = std::process::Command::new(bash);
            command
                .args(["-lc", "cd \"$1\" && shift && exec \"$@\"", "bash"])
                .arg(shell_dir)
                .arg(cmd);
            command.env("PATH", windows_augmented_path()?);
            // Autoconf is running under MSYS2 bash here, so explicitly seed tool names and PATH
            // instead of assuming the tauri-action environment preserves them.
            command.env("CC", windows_tool("CC", "clang.exe"));
            command.env("CXX", windows_tool("CXX", "clang++.exe"));
            command.env("AR", windows_tool("AR", "lib.exe"));
            command.env("LD", windows_tool("LD", "link.exe"));
            command
        } else {
            std::process::Command::new(cmd)
        };

        command.current_dir(curr_dir);

        if let Some(args) = args_opt {
            command.args(args);
        }

        let output = command.output().map_err(|e| {
            anyhow!(
                "Error running command '{}' with args '{:?}' - {:?}",
                cmd,
                args_opt,
                e
            )
        })?;

        if !output.status.success() {
            bail!(
                "Command '{}' with args '{:?}' failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
                cmd,
                args_opt,
                output.status.code(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }
}

// TODO: Consider fixing this with the upstream.
// https://github.com/rust-lang/rust-bindgen/issues/1089
// https://github.com/rust-lang/rust-bindgen/issues/1301
fn derive_serde(binding_file: &Path) -> Result<()> {
    let mut contents = String::new();
    File::open(binding_file)?.read_to_string(&mut contents)?;

    let new_contents = format!(
        "use serde::{{Serialize, Deserialize}};\n{}",
        Regex::new(r"#\s*\[\s*derive\s*\((?P<d>[^)]+)\)\s*\]\s*pub\s*(?P<s>struct|enum)")?
            .replace_all(&contents, "#[derive($d, Serialize, Deserialize)] pub $s")
    );

    File::create(&binding_file)?.write_all(new_contents.as_bytes())?;

    Ok(())
}

fn main() -> Result<()> {
    webrtc::build_if_necessary()?;
    let (webrtc_include, webrtc_lib) = webrtc::get_build_paths()?;

    let mut cc_build = cc::Build::new();

    // set mac minimum version
    if cfg!(target_os = "macos") {
        let min_version = match env::var(DEPLOYMENT_TARGET_VAR) {
            Ok(ver) => ver,
            Err(_) => {
                String::from(
                    match std::env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
                        "x86_64" => "10.10", // Using what I found here https://github.com/webrtc-uwp/chromium-build/blob/master/config/mac/mac_sdk.gni#L17
                        "aarch64" => "11.0", // Apple silicon started here.
                        arch => panic!("unknown arch: {}", arch),
                    },
                )
            }
        };

        // `cc` doesn't try to pick up on this automatically, but `clang` needs it to
        // generate a "correct" Objective-C symbol table which better matches XCode.
        // See https://github.com/h4llow3En/mac-notification-sys/issues/45.
        cc_build.flag(&format!("-mmacos-version-min={}", min_version));
    }

    cc_build
        .cpp(true)
        .file("src/wrapper.cpp")
        .include(&webrtc_include)
        .flag("-Wno-unused-parameter")
        .flag("-Wno-deprecated-declarations")
        .flag("-std=c++11")
        .out_dir(&out_dir())
        .compile("webrtc_audio_processing_wrapper");

    println!("cargo:rustc-link-search=native={}", webrtc_lib.display());
    println!("cargo:rustc-link-lib=static=webrtc_audio_processing_wrapper");

    println!("cargo:rerun-if-env-changed={}", DEPLOYMENT_TARGET_VAR);

    if cfg!(feature = "bundled") {
        println!("cargo:rustc-link-lib=static=webrtc_audio_processing");
    } else {
        println!("cargo:rustc-link-lib=dylib=webrtc_audio_processing");
    }

    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=dylib=c++");
    } else {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }

    let binding_file = out_dir().join("bindings.rs");
    bindgen::Builder::default()
        .header("src/wrapper.hpp")
        .generate_comments(true)
        .rustified_enum(".*")
        .derive_debug(true)
        .derive_default(true)
        .derive_partialeq(true)
        .clang_arg(&format!("-I{}", &webrtc_include.display()))
        .disable_name_namespacing()
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file(&binding_file)
        .expect("Couldn't write bindings!");

    if cfg!(feature = "derive_serde") {
        derive_serde(&binding_file).expect("Failed to modify derive macros");
    }

    Ok(())
}
