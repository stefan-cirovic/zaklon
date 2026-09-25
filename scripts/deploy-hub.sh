#!/usr/bin/env bash
# Deploy a release build of the hub (and optionally an APK) to a Windows test
# machine over SSH and (re)start it as a scheduled task that runs at logon.
#
#   scripts/deploy-hub.sh <ssh-host> <remote-dir> [apk-file]
#
# Example: scripts/deploy-hub.sh fly 'E:\zaklon' path/to/app-universal-debug.apk
# Remote layout: <remote-dir>\bin\zaklon-hub.exe and <remote-dir>\data (the hub's root folder).
set -euo pipefail

host="${1:?ssh host}"
remote="${2:?remote dir, e.g. E:\zaklon}"
apk="${3:-}"
exe="target/release/zaklon-hub.exe"
here="$(cd "$(dirname "$0")" && pwd)"

[ -f "$exe" ] || { echo "build first: cargo build --release -p zaklon-hub" >&2; exit 1; }
remote_fwd="${remote//\//}"

echo "stopping hub on $host (if running)"
ssh "$host" "Stop-Process -Name zaklon-hub -Force -ErrorAction SilentlyContinue; exit 0"

echo "copying hub binary and task script"
scp -q "$exe" "$host:$remote_fwd/bin/zaklon-hub.exe"
scp -q "$here/remote/hub-task.ps1" "$host:$remote_fwd/bin/hub-task.ps1"
if [ -n "$apk" ]; then
  echo "copying APK"
  scp -q "$apk" "$host:$remote_fwd/data/library/apk/zaklon.apk"
fi

echo "registering the scheduled task and starting the hub"
ssh "$host" "powershell -NoProfile -ExecutionPolicy Bypass -File ${remote}\bin\hub-task.ps1 -Root ${remote}"
