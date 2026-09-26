#!/usr/bin/env bash
# Install the Windows installer build of Zaklon on a test machine over SSH,
# replacing the standalone hub, keeping <remote-dir>\data.
#   scripts/deploy-app.sh <ssh-host> <remote-dir> [apk-file]
set -euo pipefail
host="${1:?ssh host}"; remote="${2:?remote dir}"; apk="${3:-}"
here="$(cd "$(dirname "$0")" && pwd)"
setup="$(ls -t "$here"/../target/release/bundle/nsis/*-setup.exe | head -1)"
remote_fwd="${remote//\//}"
echo "copying $(basename "$setup")"
scp -q "$setup" "$host:$remote_fwd/Zaklon-setup.exe"
scp -q "$here/remote/install-app.ps1" "$host:$remote_fwd/install-app.ps1"
if [ -n "$apk" ]; then
  ssh "$host" "New-Item -ItemType Directory -Force '${remote}\data\library\apk' | Out-Null"
  scp -q "$apk" "$host:$remote_fwd/data/library/apk/zaklon.apk"
fi
ssh "$host" "powershell -NoProfile -ExecutionPolicy Bypass -File ${remote}\install-app.ps1 -Root ${remote} -Installer ${remote}\Zaklon-setup.exe"
