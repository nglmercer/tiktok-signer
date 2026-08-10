#!/bin/sh
# Start the virtual display, then hand the process over to the server.
#
# `exec` at the end is the point: the server becomes the container's main process, so the
# stop signal reaches it directly and its graceful shutdown runs. Wrapping it in `xvfb-run`
# would put a shell in that path instead. The signal is SIGINT — see `STOPSIGNAL`.
set -eu

: "${DISPLAY:=:99}"
export DISPLAY

if [ ! -e "/tmp/.X11-unix/X${DISPLAY#:}" ]; then
    Xvfb "$DISPLAY" -screen 0 "${TTL_SCREEN:-1280x800x24}" -nolisten tcp &

    # The WebView fails to build if it starts before the display accepts connections.
    i=0
    while [ ! -e "/tmp/.X11-unix/X${DISPLAY#:}" ]; do
        i=$((i + 1))
        if [ "$i" -gt 100 ]; then
            echo "Xvfb did not come up on $DISPLAY" >&2
            exit 1
        fi
        sleep 0.1
    done
fi

exec "$@"
