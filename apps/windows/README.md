# Photosink for Windows

WinUI 3 front end over the `photosink-bridge` crate. Behaviour lives in the
bridge, which returns typed data; this app owns the presentation: every label,
message and number or date format is built here (`Copy.cs`).

## Prerequisites

- Rust with the MSVC toolchain (`rustup default stable-msvc`); for ARM64 builds also
  `rustup target add aarch64-pc-windows-msvc` and the Visual Studio ARM64 build tools.
- .NET 10 SDK.
- Windows App SDK 2.5 (pulled from NuGet by the build; the app is self-contained and
  unpackaged, so no runtime install is needed).
- uniffi-bindgen-cs matching uniffi 0.31:

  ```powershell
  cargo install uniffi-bindgen-cs --git https://github.com/NordSecurity/uniffi-bindgen-cs --tag v0.11.1+v0.31.0
  ```

## Build and run

```powershell
.\build.ps1                                    # host architecture, Release
.\build.ps1 -Target aarch64-pc-windows-msvc    # ARM64
.\build.ps1 -Publish                           # self-contained app in bin\publish\<rid>
```

`build.ps1` builds the bridge DLL, regenerates `Generated\photosink_bridge.cs`
(gitignored) and builds the app with the DLL copied next to `Photosink.exe`.
Run `bin\<Platform>\Release\net10.0-windows10.0.19041.0\<rid>\Photosink.exe`, or
open `Photosink.csproj` in Visual Studio after running `build.ps1` once.
