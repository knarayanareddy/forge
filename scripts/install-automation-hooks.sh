#!/usr/bin/env bash
# Darwin launchd/cron stub for AetherForge automation triggers (Phase 7 slice 7.1).
# Installs a user LaunchAgent that POSTs automation_tick to the daemon every 30 minutes.
set -euo pipefail

DAEMON_ADDR="${AETHER_DAEMON_ADDR:-127.0.0.1:3847}"
PLIST_LABEL="com.aetherforge.automation-tick"
PLIST_PATH="${HOME}/Library/LaunchAgents/${PLIST_LABEL}.plist"

mkdir -p "${HOME}/Library/LaunchAgents"

cat > "${PLIST_PATH}" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>${PLIST_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/bin/curl</string>
    <string>-sf</string>
    <string>-X</string>
    <string>POST</string>
    <string>tcp://${DAEMON_ADDR}</string>
    <string>-d</string>
    <string>{"method":"automation_tick","params":{}}</string>
  </array>
  <key>StartInterval</key>
  <integer>1800</integer>
  <key>RunAtLoad</key>
  <false/>
</dict>
</plist>
EOF

launchctl unload "${PLIST_PATH}" 2>/dev/null || true
launchctl load "${PLIST_PATH}"

echo "Installed ${PLIST_PATH} (30m automation_tick stub → ${DAEMON_ADDR})"
