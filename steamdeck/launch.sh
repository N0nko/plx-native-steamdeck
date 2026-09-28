#!/bin/sh
set -eu
umask 077
unset PLXNATIVE_MPV_TEST_LOG
root="$HOME/.local/share/plxnative-steamdeck"
export LD_LIBRARY_PATH="$root/assets${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export SDL_VIDEODRIVER=x11
export SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS=1
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
if [ -S "$XDG_RUNTIME_DIR/gamescope-0" ]; then
    export DISPLAY=:1
    export GAMESCOPE_WAYLAND_DISPLAY=gamescope-0
fi
exec "$root/bin/plxnative-deck" "$@"
