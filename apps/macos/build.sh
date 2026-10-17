#!/bin/sh
# Builds Photosink.app into build/.
#
#   UNIVERSAL=1      build for arm64 and x86_64 (default: this Mac's architecture)
#   SIGN_IDENTITY=…  sign with this identity, hardened runtime and entitlements
#                    (default: ad-hoc)
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
universal=${UNIVERSAL:-0}
identity=${SIGN_IDENTITY:--}

cd "$root"
if [ "$universal" = 1 ]; then
    cargo build --release -p photosink-bridge --target aarch64-apple-darwin
    cargo build --release -p photosink-bridge --target x86_64-apple-darwin
    mkdir -p target/universal/release
    lipo -create -output target/universal/release/libphotosink_bridge.a \
        target/aarch64-apple-darwin/release/libphotosink_bridge.a \
        target/x86_64-apple-darwin/release/libphotosink_bridge.a
    export PHOTOSINK_RUST_LIB="$root/target/universal/release"
    bindings_lib=target/aarch64-apple-darwin/release/libphotosink_bridge.dylib
    swift_arch="--arch arm64 --arch x86_64"
else
    cargo build --release -p photosink-bridge
    export PHOTOSINK_RUST_LIB="$root/target/release"
    bindings_lib=target/release/libphotosink_bridge.dylib
    swift_arch=""
fi
cargo run -q --release -p photosink-bridge --features cli --bin uniffi-bindgen -- \
    generate --library "$bindings_lib" --language swift --out-dir "$here/build/bindings"

mkdir -p "$here/Sources/Photosink/Generated" "$here/Sources/photosink_bridgeFFI/include"
cp "$here/build/bindings/photosink_bridge.swift" "$here/Sources/Photosink/Generated/"
cp "$here/build/bindings/photosink_bridgeFFI.h" "$here/Sources/photosink_bridgeFFI/include/"
cp "$here/build/bindings/photosink_bridgeFFI.modulemap" "$here/Sources/photosink_bridgeFFI/include/module.modulemap"

cd "$here"
# shellcheck disable=SC2086
swift build -c release $swift_arch
# shellcheck disable=SC2086
bin=$(swift build -c release $swift_arch --show-bin-path)

app="$here/build/Photosink.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin/Photosink" "$app/Contents/MacOS/Photosink"
cp "$here/Resources/Info.plist" "$app/Contents/Info.plist"
cp "$here/Resources/icon.icns" "$app/Contents/Resources/icon.icns"
if [ "$identity" = "-" ]; then
    codesign --force --sign - "$app"
else
    codesign --force --timestamp --options runtime \
        --entitlements "$here/Resources/Photosink.entitlements" --sign "$identity" "$app"
fi
echo "$app"
