#!/bin/sh
# Build "IDE by Insyd.app" and IDE-by-Insyd-<version>.dmg in target/release/bundle.
#
#   packaging/macos/bundle.sh            # runtime-compiled shaders (no Metal toolchain needed)
#   PRECOMPILE_SHADERS=1 packaging/macos/bundle.sh   # needs `xcodebuild -downloadComponent MetalToolchain`
#   SIGN_IDENTITY="Developer ID Application: …" packaging/macos/bundle.sh   # real signature
set -eu
cd "$(dirname "$0")/../.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
OUT=target/release/bundle
APP="$OUT/IDE by Insyd.app"

FEATURES=""
if [ "${PRECOMPILE_SHADERS:-0}" = "1" ]; then FEATURES="--no-default-features"; fi
cargo build --release -p insyde-app $FEATURES
cargo build --release -p insyde-cli

rm -rf "$APP" && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/insyde "$APP/Contents/MacOS/insyde"
cp target/release/insy "$APP/Contents/MacOS/insy"

# Icon: render the design's mark, then build an .icns with every required size.
ICONSET="$OUT/AppIcon.iconset" && rm -rf "$ICONSET" && mkdir -p "$ICONSET"
swift packaging/macos/make-icon.swift "$OUT/icon.png"
for s in 16 32 128 256 512; do
  sips -z $s $s "$OUT/icon.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2)); sips -z $d $d "$OUT/icon.png" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

sed "s/__VERSION__/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"

# Ad-hoc signature by default (runs on this Mac); pass SIGN_IDENTITY for distribution.
codesign --force --deep --options runtime --sign "${SIGN_IDENTITY:--}" "$APP"

DMG="$OUT/IDE-by-Insyd-$VERSION.dmg"
STAGE="$OUT/dmg" && rm -rf "$STAGE" && mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/" && ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
hdiutil create -quiet -volname "IDE by Insyd" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
rm -rf "$STAGE" "$ICONSET"
echo "Built $APP"
echo "Built $DMG"
echo "CLI: ln -sf \"/Applications/IDE by Insyd.app/Contents/MacOS/insy\" /usr/local/bin/insy"
