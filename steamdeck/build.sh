#!/bin/sh
# Run in Linux/WSL from any directory. Build outputs stay out of the source checkout.
set -eu
src=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
export CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/plxnative-steamdeck-target}"
export PATH="$HOME/.cargo/bin:$PATH"
cargo +stable build --locked --release --manifest-path "$src/rust-modules/Cargo.toml" --no-default-features --features steamdeck --bin plxnative-deck
out="$src/steamdeck/dist"
mkdir -p "$out/bin" "$out/assets" "$out/vulkan"
cp "$CARGO_TARGET_DIR/release/plxnative-deck" "$out/bin/"
cp "$src"/pkg/appfont*.ttf "$out/assets/"
ttf=$(ldconfig -p | awk '/libSDL2_ttf-2.0.so.0 / {print $NF; exit}')
test -n "$ttf"
cp "$ttf" "$out/assets/libSDL2_ttf-2.0.so.0"
cp "$src/steamdeck/launch.sh" "$out/"
cp "$src/steamdeck/install.sh" "$out/"
cp "$src/steamdeck/VkLayer_FROG_gamescope_wsi.x86_64.json" "$out/vulkan/"
cp "$src/LICENSE" "$out/LICENSE"
cp "$src/pkg/OFL.txt" "$out/assets/OFL.txt"
chmod +x "$out/launch.sh" "$out/bin/plxnative-deck"
printf 'Built %s\n' "$out"
