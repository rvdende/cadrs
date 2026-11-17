#!/usr/bin/env bash
# Packages a built executable into dist/ as a release archive:
#   package.sh linux <cadrs>       → dist/cadrs-v<version>-x86_64-linux.tar.gz
#   package.sh windows <cadrs.exe> → dist/cadrs-v<version>-x86_64-windows.zip and the installer
#                                    dist/cadrs-v<version>-x86_64-windows-setup.exe (NSIS's
#                                    makensis, tools/windows/cadrs.nsi); in two steps, so the
#                                    release can sign cadrs.exe between them
#                                    (tools/windows/README.md):
#   package.sh windows-folder <cadrs.exe> → dist/cadrs-v<version>-x86_64-windows/
#   package.sh windows-pack               → the zip and the installer of that folder
#   package.sh macos <cadrs>       → dist/cadrs-v<version>-universal-macos.dmg (on macOS, from a
#                                    universal Apple silicon and Intel executable)
#   package.sh web <dir>           → dist/cadrs-v<version>-web.tar.gz (the browser build's folder,
#                                    tools/web/build.sh; the site at https://cadrs.net)
# Each holds the stripped executable, the built-in libraries/, README.md and the licenses; the
# macOS one holds them as the app bundle cadrs.app (`cadrs --bundle`, as `cadrs --install` makes
# it), ad-hoc signed, beside a link to /Applications to drag it to. Used by
# tools/release/build.sh and .github/workflows/release.yml.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
platform="$1"
exe="${2:-}"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
step=all
case "$platform" in
    windows-folder) platform=windows step=folder ;;
    windows-pack) platform=windows step=pack ;;
esac
name="cadrs-v$version-x86_64-$platform"
dist="$root/dist"
dir="$dist/$name"

case "$platform" in
    web)
        name="cadrs-v$version-web"
        dir="$dist/$name"
        rm -rf "$dir"
        mkdir -p "$dir"
        cp -r "$exe"/. "$dir/"
        cp LICENSE-APACHE LICENSE-MIT "$dir/"
        tar -C "$dist" -czf "$dist/$name.tar.gz" "$name"
        ls -l "$dist/$name.tar.gz"
        exit 0
        ;;
    linux) exe_name=cadrs strip=strip ;;
    windows) exe_name=cadrs.exe strip=x86_64-w64-mingw32-strip ;;
    macos)
        name="cadrs-v$version-universal-macos"
        dir="$dist/$name"
        rm -rf "$dir" "$dist/$name.dmg"
        mkdir -p "$dir"
        # Stripped, then signed again (ad hoc: no Apple Developer ID yet): Apple silicon runs
        # nothing unsigned, and stripping drops the linker's signature.
        tmp="$dist/cadrs-macos-bin"
        cp "$exe" "$tmp"
        strip -x "$tmp"
        codesign --force --sign - "$tmp"
        "$tmp" --bundle "$dir/cadrs.app" --libraries "$root/libraries"
        rm -f "$tmp"
        # The executable is the bundle's only code (`libraries/` is in Contents/Resources), so
        # no --deep: that signs whatever looks like nested code.
        codesign --force --sign - "$dir/cadrs.app"
        codesign --verify --deep --strict "$dir/cadrs.app"
        cp README.md LICENSE-APACHE LICENSE-MIT "$dir/"
        ln -s /Applications "$dir/Applications"
        hdiutil create -volname "cadrs $version" -srcfolder "$dir" -ov -format UDZO "$dist/$name.dmg"
        ls -l "$dist/$name.dmg"
        exit 0
        ;;
    *) echo "unknown platform: $platform" >&2; exit 1 ;;
esac

if [[ "$step" != pack ]]; then
    rm -rf "$dir"
    mkdir -p "$dir"
    cp "$exe" "$dir/$exe_name"
    "$strip" "$dir/$exe_name"
    cp README.md LICENSE-APACHE LICENSE-MIT "$dir/"
    # The built-in symbols, footprints and 3D models: the app reads them from `libraries/`
    # beside the executable.
    cp -r libraries "$dir/"
fi
if [[ "$step" == folder ]]; then
    ls -l "$dir/$exe_name"
    exit 0
fi
[[ -f "$dir/$exe_name" ]] || { echo "no $dir/$exe_name: run package.sh windows-folder first" >&2; exit 1; }

cd "$dist"
case "$platform" in
    linux) rm -f "$name.tar.gz" && tar czf "$name.tar.gz" "$name" ;;
    windows)
        rm -f "$name.zip" && zip -qr "$name.zip" "$name"
        # The installer of the same files. Its version resource takes four numbers, so a
        # pre-release's suffix (0.2.9-beta.1) is dropped there.
        nums="${version%%-*}"
        rm -f "$name-setup.exe"
        makensis -V2 -NOCD \
            -DVERSION="$version" -DVIVERSION="$nums.0" \
            -DSRC="$dir" -DOUT="$dist/$name-setup.exe" \
            "$root/tools/windows/cadrs.nsi"
        ls -l "$name-setup.exe"
        ;;
esac
ls -l "$dist/$name".*
