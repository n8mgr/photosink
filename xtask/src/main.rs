use anyhow::{Context, Result, bail, ensure};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const HELP: &str = "Usage: cargo xtask build [OPTIONS]

Build the native app for the current platform in Release mode.

Options:
  --universal                    macOS: build for arm64 and x86_64
  --target <TRIPLE>               Windows or Linux: Rust target triple
  --configuration <Debug|Release> Windows: .NET configuration (default: Release)
  --publish                      Windows: publish a self-contained app
  -h, --help                     Show this help

Set SIGN_IDENTITY to sign the macOS app with a certificate (default: ad-hoc).";

#[derive(Default)]
struct BuildOptions {
    universal: bool,
    target: Option<String>,
    configuration: Option<String>,
    publish: bool,
}

impl BuildOptions {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut options = Self::default();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--universal" => options.universal = true,
                "--publish" => options.publish = true,
                "--target" | "--configuration" => {
                    let value = args
                        .next()
                        .filter(|value| !value.starts_with('-'))
                        .with_context(|| format!("{arg} requires a value"))?;
                    if arg == "--target" {
                        options.target = Some(value);
                    } else {
                        ensure!(
                            matches!(value.as_str(), "Debug" | "Release"),
                            "--configuration must be Debug or Release"
                        );
                        options.configuration = Some(value);
                    }
                }
                _ => bail!("unknown option: {arg}\n\n{HELP}"),
            }
        }
        Ok(options)
    }

    fn validate(&self, platform: &str) -> Result<()> {
        ensure!(
            !self.universal || platform == "macos",
            "--universal is only supported on macOS"
        );
        ensure!(
            self.target.is_none() || matches!(platform, "windows" | "linux"),
            "--target is only supported on Windows and Linux"
        );
        ensure!(
            (!self.publish && self.configuration.is_none()) || platform == "windows",
            "--publish and --configuration are only supported on Windows"
        );
        if let Some(target) = &self.target {
            match platform {
                "windows" => {
                    windows_target(target)?;
                }
                "linux" => ensure!(target.contains("-linux-"), "expected a Linux target triple"),
                _ => {}
            }
        }
        Ok(())
    }
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        None | Some("-h" | "--help") => {
            println!("{HELP}");
            return Ok(());
        }
        Some("build") => {}
        Some(task) => bail!("unknown task: {task}\n\n{HELP}"),
    }
    let args: Vec<_> = args.collect();
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        println!("{HELP}");
        return Ok(());
    }
    let options = BuildOptions::parse(args.into_iter())?;
    options.validate(env::consts::OS)?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    match env::consts::OS {
        "macos" => build_macos(root, &options),
        "windows" => build_windows(root, &options),
        "linux" => build_linux(root, &options),
        platform => bail!("unsupported platform: {platform}"),
    }
}

fn cargo(root: &Path) -> Command {
    let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.current_dir(root);
    command
}

fn run(command: &mut Command) -> Result<()> {
    eprintln!("==> {command:?}");
    let status = command
        .status()
        .with_context(|| format!("could not run {command:?}"))?;
    ensure!(status.success(), "{command:?} failed ({status})");
    Ok(())
}

fn output(command: &mut Command) -> Result<String> {
    let output = command
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("could not run {command:?}"))?;
    ensure!(
        output.status.success(),
        "{command:?} failed ({})",
        output.status
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<()> {
    let (from, to) = (from.as_ref(), to.as_ref());
    fs::create_dir_all(to.parent().context("missing destination directory")?)?;
    fs::copy(from, to)
        .with_context(|| format!("could not copy {} to {}", from.display(), to.display()))?;
    Ok(())
}

fn remove_dir(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("could not remove {}", path.display())),
    }
}

fn build_bridge(root: &Path, target: Option<&str>) -> Result<PathBuf> {
    let target_dir = root.join("target");
    let mut command = cargo(root);
    command
        .args([
            "build",
            "--release",
            "-p",
            "photosink-bridge",
            "--target-dir",
        ])
        .arg(&target_dir);
    let mut artifacts = target_dir;
    if let Some(target) = target {
        command.args(["--target", target]);
        artifacts.push(target);
    }
    run(&mut command)?;
    Ok(artifacts.join("release"))
}

fn build_macos(root: &Path, options: &BuildOptions) -> Result<()> {
    let here = root.join("apps/macos");
    let (rust_lib, bindings_lib) = if options.universal {
        let arm = build_bridge(root, Some("aarch64-apple-darwin"))?;
        let intel = build_bridge(root, Some("x86_64-apple-darwin"))?;
        let universal = root.join("target/universal/release");
        fs::create_dir_all(&universal)?;
        run(Command::new("lipo")
            .args(["-create", "-output"])
            .arg(universal.join("libphotosink_bridge.a"))
            .arg(arm.join("libphotosink_bridge.a"))
            .arg(intel.join("libphotosink_bridge.a")))?;
        (universal, arm.join("libphotosink_bridge.dylib"))
    } else {
        let native = build_bridge(root, None)?;
        let bindings_lib = native.join("libphotosink_bridge.dylib");
        (native, bindings_lib)
    };
    let bindings = here.join("build/bindings");
    run(cargo(root)
        .args([
            "run",
            "-q",
            "--release",
            "-p",
            "photosink-bridge",
            "--features",
            "cli",
            "--bin",
            "uniffi-bindgen",
            "--",
            "generate",
            "--library",
        ])
        .arg(bindings_lib)
        .args(["--language", "swift", "--out-dir"])
        .arg(&bindings))?;
    for (source, destination) in [
        (
            "photosink_bridge.swift",
            "Sources/Photosink/Generated/photosink_bridge.swift",
        ),
        (
            "photosink_bridgeFFI.h",
            "Sources/photosink_bridgeFFI/include/photosink_bridgeFFI.h",
        ),
        (
            "photosink_bridgeFFI.modulemap",
            "Sources/photosink_bridgeFFI/include/module.modulemap",
        ),
    ] {
        copy(bindings.join(source), here.join(destination))?;
    }

    let mut swift = Command::new("swift");
    swift
        .current_dir(&here)
        .env("PHOTOSINK_RUST_LIB", rust_lib)
        .args(["build", "-c", "release"]);
    if options.universal {
        swift.args(["--arch", "arm64", "--arch", "x86_64"]);
    }
    run(&mut swift)?;
    let bin = PathBuf::from(output(swift.arg("--show-bin-path"))?);
    let app = here.join("build/Photosink.app");
    remove_dir(&app)?;
    copy(bin.join("Photosink"), app.join("Contents/MacOS/Photosink"))?;
    copy(
        here.join("Resources/Info.plist"),
        app.join("Contents/Info.plist"),
    )?;
    copy(
        here.join("Resources/icon.icns"),
        app.join("Contents/Resources/icon.icns"),
    )?;
    let identity = env::var_os("SIGN_IDENTITY")
        .filter(|identity| !identity.is_empty())
        .unwrap_or_else(|| "-".into());
    let mut codesign = Command::new("codesign");
    codesign.arg("--force");
    if identity != "-" {
        codesign
            .args(["--timestamp", "--options", "runtime", "--entitlements"])
            .arg(here.join("Resources/Photosink.entitlements"));
    }
    run(codesign.arg("--sign").arg(identity).arg(&app))?;
    println!("{}", app.display());
    Ok(())
}

fn windows_target(target: &str) -> Result<(&'static str, &'static str)> {
    match target {
        "x86_64-pc-windows-msvc" => Ok(("x64", "win-x64")),
        "aarch64-pc-windows-msvc" => Ok(("ARM64", "win-arm64")),
        _ => bail!("Windows target must be x86_64-pc-windows-msvc or aarch64-pc-windows-msvc"),
    }
}

fn build_windows(root: &Path, options: &BuildOptions) -> Result<()> {
    let target = match options.target.as_deref() {
        Some(target) => target,
        None => match env::consts::ARCH {
            "aarch64" => "aarch64-pc-windows-msvc",
            "x86_64" => "x86_64-pc-windows-msvc",
            arch => bail!("unsupported Windows architecture: {arch}"),
        },
    };
    let (platform, rid) = windows_target(target)?;
    let here = root.join("apps/windows");
    let dll = build_bridge(root, Some(target))?.join("photosink_bridge.dll");
    let generated = here.join("Generated");
    remove_dir(&generated)?;
    run(Command::new("uniffi-bindgen-cs")
        .current_dir(root)
        .arg("--library")
        .arg(&dll)
        .arg("--out-dir")
        .arg(generated))?;

    let mut dotnet = Command::new("dotnet");
    dotnet
        .current_dir(root)
        .arg(if options.publish { "publish" } else { "build" })
        .arg(here.join("Photosink.csproj"))
        .args([
            "-c",
            options.configuration.as_deref().unwrap_or("Release"),
            "-r",
            rid,
        ])
        .arg(format!("-p:Platform={platform}"))
        .arg(format!("-p:BridgeTriple={target}"));
    if options.publish {
        let out = here.join("bin/publish").join(rid);
        run(dotnet.args(["--self-contained", "-o"]).arg(&out))?;
        println!("Published to {}", out.display());
    } else {
        run(&mut dotnet)?;
    }
    Ok(())
}

fn build_linux(root: &Path, options: &BuildOptions) -> Result<()> {
    let here = root.join("apps/linux");
    let mut command = cargo(root);
    command
        .args(["build", "--release", "--manifest-path"])
        .arg(here.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(here.join("target"));
    if let Some(target) = &options.target {
        command.args(["--target", target]);
    }
    run(&mut command)
}
