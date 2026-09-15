#!/bin/sh
# Coolify/Docker entrypoint: the main `--serve` process (the browser app, the JSON/snapshot API,
# the CORS proxy) plus the developer-log admin panel, in one container.
#
# Two long-lived processes, no supervisor: `--devlog-serve` starts in the background and `--serve`
# is `exec`'d in its place as PID 1, so Docker's stop signal still reaches the process that
# actually matters (the one serving traffic) directly. The backgrounded admin panel is a
# diagnostic aid, not the thing being load-balanced or health-checked — losing it a moment late on
# shutdown costs nothing a redeploy doesn't already reset.
#
# HOOKECHO_DEVLOG_PORT / HOOKECHO_DEVLOG_TOKEN configure the admin panel; HOOKECHO_DEVLOG=1 (set by
# default below unless already provided) makes this same `--serve` process ship its own logs to it
# automatically. See CHANGELOG.md's devlog entry, or `hookecho --devlog-serve --help`-equivalent
# comments in main.rs, for the whole picture.
set -e

DEVLOG_PORT="${HOOKECHO_DEVLOG_PORT:-8884}"
devlog_args="--devlog-serve $DEVLOG_PORT --bind 0.0.0.0"
if [ -n "$HOOKECHO_DEVLOG_TOKEN" ]; then
    devlog_args="$devlog_args --devlog-token $HOOKECHO_DEVLOG_TOKEN"
    # The token gate applies to this process's own shipped logs too — a bare `HOOKECHO_DEVLOG=1`
    # would otherwise post to a locked-down panel and get 401'd forever.
    export HOOKECHO_DEVLOG="${HOOKECHO_DEVLOG:-http://127.0.0.1:${DEVLOG_PORT}/ingest?token=${HOOKECHO_DEVLOG_TOKEN}}"
else
    export HOOKECHO_DEVLOG="${HOOKECHO_DEVLOG:-1}"
fi
# shellcheck disable=SC2086 # word-splitting is the point: this is a plain argv, not one string
hookecho $devlog_args &

exec hookecho --serve 8080 --bind 0.0.0.0 --web-root /app/web
