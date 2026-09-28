#!/bin/sh
# Run on Steam Deck beside the staged files from build.sh. No system-image changes.
set -eu
umask 077
src=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
dest="$HOME/.local/share/plxnative-steamdeck"
flatpak info io.mpv.Mpv >/dev/null 2>&1 || flatpak install --user -y flathub io.mpv.Mpv
mkdir -p "$dest/bin" "$dest/assets" "$dest/vulkan" "$HOME/.local/share/applications"
if [ "$src" != "$dest" ]; then
    cp "$src/bin/plxnative-deck" "$dest/bin/"
    cp "$src/assets/"* "$dest/assets/"
    cp "$src/vulkan/"* "$dest/vulkan/"
    cp "$src/launch.sh" "$src/LICENSE" "$dest/"
fi
chmod +x "$dest/launch.sh" "$dest/bin/plxnative-deck"
cat > "$HOME/.local/share/applications/plxnative-steamdeck.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=PlxNative Steam Deck
Comment=Plex with native controls, HDR10 and offline downloads
Exec=$dest/launch.sh
Terminal=false
Categories=AudioVideo;Video;Player;
Actions=Downloads;

[Desktop Action Downloads]
Name=Offline downloads
Exec=$dest/launch.sh --downloads
EOF
printf 'Installed. Add %s/launch.sh as a non-Steam game for Gaming Mode.\n' "$dest"
