#!/usr/bin/env bash
# Kills any running debug/release build of the app, e.g. a leftover instance from a prior
# run-app.sh before relaunching. Matches by binary path, not by name, so it won't catch
# unrelated processes.
set -euo pipefail

pids="$(pgrep -f 'target/(debug|release)/app( |$)' || true)"
if [ -z "$pids" ]; then
    echo "no running app instance"
    exit 0
fi

echo "killing: $pids"
kill $pids
