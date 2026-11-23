#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${1:-$ROOT/release/macos}"
APP="$DEST/Minecraft桌宠.app"
VERSION="$(/usr/libexec/PlistBuddy -c 'Print :MinecraftPetReleaseVersion' "$ROOT/macos/Info.plist")"
[[ "$VERSION" =~ ^[0-9]+(\.[0-9]+){2,3}$ ]] || { echo "Invalid macOS release version: $VERSION" >&2; exit 1; }
mkdir -p "$DEST" "$ROOT/macos/build"
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$HOME=/build-user"
# Intel needs macOS 13: Command Line Tools 27 no longer ship an x86_64 Swift 5.6
# compatibility library, which macOS 12 builds link. Info.plist states both minimums.
for PAIR in 'aarch64-apple-darwin arm64 12.0' 'x86_64-apple-darwin x86_64 13.0'; do
    read -r RUST_TARGET SWIFT_ARCH MIN_MACOS <<< "$PAIR"
    export MACOSX_DEPLOYMENT_TARGET="$MIN_MACOS"
    cargo build --manifest-path "$ROOT/native/Cargo.toml" --locked --release --lib --target "$RUST_TARGET"
    swiftc -O -file-prefix-map "$HOME=/build-user" -swift-version 5 -target "$SWIFT_ARCH-apple-macosx$MIN_MACOS" \
        -import-objc-header "$ROOT/macos/PetCore.h" "$ROOT/macos/main.swift" "$ROOT/macos/InputController.swift" "$ROOT/macos/PlatformSupport.swift" "$ROOT/macos/SelfTests.swift" "$ROOT/macos/Console.swift" "$ROOT/macos/ConsoleSession.swift" "$ROOT/macos/MacFeatures.swift" "$ROOT/macos/RegressionTests.swift" "$ROOT/macos/PermissionGuide.swift" "$ROOT/macos/PetMenu.swift" "$ROOT/macos/CapePicker.swift" "$ROOT/macos/CaptureWindow.swift" "$ROOT/macos/MetalTest.swift" \
        "$ROOT/native/target/$RUST_TARGET/release/libpet_core.a" \
        -framework Metal -framework IOSurface -framework CoreAudio -framework Carbon -framework AppKit -framework ApplicationServices -framework Security -framework SystemConfiguration \
        -o "$ROOT/macos/build/MinecraftPet-$SWIFT_ARCH"
done
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create "$ROOT/macos/build/MinecraftPet-arm64" "$ROOT/macos/build/MinecraftPet-x86_64" -output "$APP/Contents/MacOS/MinecraftPet"
# Remove linker object-path debug symbols before signing.
xcrun strip -S "$APP/Contents/MacOS/MinecraftPet"
cp "$ROOT/macos/Info.plist" "$APP/Contents/Info.plist"
cp "$ROOT/LICENSE" "$ROOT/THIRD_PARTY_NOTICES.md" "$ROOT/macos/使用说明-macOS.md" "$APP/Contents/Resources/"
cp -R "$ROOT/native/licenses" "$APP/Contents/Resources/"
mkdir -p "$APP/Contents/Resources/assets"
cp "$ROOT/native/assets/apple-provenance.json" "$APP/Contents/Resources/assets/"
cp "$ROOT/native/assets/capes/catalog.json" "$APP/Contents/Resources/assets/cape-catalog.json"
cp "$ROOT/native/assets/decorations/SOURCES.md" "$APP/Contents/Resources/assets/decoration-sources.md"
cp "$ROOT/native/assets/p4-sources.txt" "$APP/Contents/Resources/assets/"
cp "$ROOT/native/assets/pat-meow.wav" "$ROOT/native/assets/pat-meow-high.wav" "$APP/Contents/Resources/"
cp "$ROOT/assets/github.svg" "$APP/Contents/Resources/"
cp "$ROOT/assets/default-skin.png" "$APP/Contents/Resources/"
ICONSET="$ROOT/macos/build/AppIcon.iconset"
mkdir -p "$ICONSET"
sips -s format png "$ROOT/assets/icon.ico" --out "$ROOT/macos/build/icon.png" >/dev/null
for SIZE in 16 32 128 256 512; do
    sips -z "$SIZE" "$SIZE" "$ROOT/macos/build/icon.png" --out "$ICONSET/icon_${SIZE}x${SIZE}.png" >/dev/null
    DOUBLE=$((SIZE * 2))
    sips -z "$DOUBLE" "$DOUBLE" "$ROOT/macos/build/icon.png" --out "$ICONSET/icon_${SIZE}x${SIZE}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
codesign --force --sign - "$APP"
codesign --verify --deep --strict "$APP"
plutil -lint "$APP/Contents/Info.plist"
lipo -info "$APP/Contents/MacOS/MinecraftPet"
cp "$ROOT/macos/使用说明-macOS.md" "$DEST/使用说明-macOS.md"
if [ -f "$ROOT/macos/更新与验证-$VERSION.md" ]; then
    cp "$ROOT/macos/更新与验证-$VERSION.md" "$DEST/更新与验证.md"
fi
# A single installer contains both architectures. Mount and drag the signed app to Applications.
STAGING="$(mktemp -d "${TMPDIR:-/tmp}/pet-dmg.XXXXXX")"
trap 'rm -rf "$STAGING"' EXIT
ditto "$APP" "$STAGING/Minecraft桌宠.app"
ln -s /Applications "$STAGING/Applications"
DMG="$DEST/Minecraft-Desktop-Pet-${VERSION}-macOS-Universal.dmg"
hdiutil create -ov -fs HFS+ -format UDZO -volname "Minecraft桌宠 $VERSION" -srcfolder "$STAGING" "$DMG"
hdiutil verify "$DMG"
echo "Built: $APP"
echo "Built: $DMG"
