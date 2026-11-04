#!/usr/bin/env bash
set -euo pipefail

version=${1#v}
version=${version/-/\~}
target=$2
output=$(realpath -m "$3")
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)

case "$target" in
  x86_64-unknown-linux-gnu) arch=amd64; multiarch=x86_64-linux-gnu ;;
  aarch64-unknown-linux-gnu) arch=arm64; multiarch=aarch64-linux-gnu ;;
  *) echo "Unsupported Debian package target: $target" >&2; exit 1 ;;
esac
dpkg --validate-version "$version"

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
umask 022
mkdir -p "$stage/debian" "$stage/package/DEBIAN" "$(dirname "$output")"
install -Dm755 "$root/apps/linux/target/$target/release/photosink" "$stage/package/usr/bin/photosink"
install -Dm644 "$root/apps/linux/photosink.desktop" "$stage/package/usr/share/applications/com.n8mgr.photosink.desktop"
install -Dm644 "$root/apps/linux/photosink.png" "$stage/package/usr/share/icons/hicolor/512x512/apps/com.n8mgr.photosink.png"
install -Dm644 "$root/LICENSE" "$stage/package/usr/share/doc/photosink/copyright"
install -Dm644 "$root/README.md" "$stage/package/usr/share/doc/photosink/README.md"

cat > "$stage/debian/control" <<EOF
Source: photosink
Maintainer: Photosink contributors <n8mgr@users.noreply.github.com>

Package: photosink
Architecture: $arch
Description: Export and organize photos from multiple sources
EOF
dependencies=$(
  cd "$stage"
  DEB_HOST_ARCH="$arch" dpkg-shlibdeps -O -l"/usr/lib/$multiarch" -l"/lib/$multiarch" -e"package/usr/bin/photosink"
)
dependencies=${dependencies#shlibs:Depends=}
test -n "$dependencies"

cat > "$stage/package/DEBIAN/control" <<EOF
Package: photosink
Version: $version
Section: graphics
Priority: optional
Architecture: $arch
Maintainer: Photosink contributors <n8mgr@users.noreply.github.com>
Homepage: https://github.com/n8mgr/photosink
Installed-Size: $(du -sk "$stage/package/usr" | cut -f1)
Depends: $dependencies, adwaita-icon-theme, xdg-utils
Description: Export and organize photos from multiple sources
 Export OneDrive, Dropbox and Google Takeout into one folder tree with
 metadata, and maintain that tree.
EOF
dpkg-deb --root-owner-group --build "$stage/package" "$output"
dpkg-deb --info "$output"
