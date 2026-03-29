#!/bin/bash
# install-launchagent.sh
# macOS Autostart fuer VPN Router (LaunchAgent = Pendant zu systemd user service)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
PLIST="$HOME/Library/LaunchAgents/de.vpn-router.plist"

# Find binary
BINARY="$PROJECT_DIR/../../target/release/vpn-router"
[ ! -f "$BINARY" ] && BINARY="$PROJECT_DIR/target/release/vpn-router"

if [ ! -f "$BINARY" ]; then
    echo "Binary nicht gefunden. Zuerst bauen: cargo build --release --bin vpn-router"
    exit 1
fi

mkdir -p "$HOME/Library/LaunchAgents"

cat > "$PLIST" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>de.vpn-router</string>
    <key>ProgramArguments</key>
    <array>
        <string>${BINARY}</string>
    </array>
    <key>WorkingDirectory</key>
    <string>${PROJECT_DIR}</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>ROUTER_IP</key>
        <string>192.168.8.1</string>
        <key>BIND_ADDR</key>
        <string>127.0.0.1:3080</string>
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/vpn-router.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/vpn-router.log</string>
</dict>
</plist>
EOF

launchctl unload "$PLIST" 2>/dev/null || true
launchctl load "$PLIST"

echo "LaunchAgent installiert und gestartet."
echo "  Status: launchctl list | grep vpn-router"
echo "  Stoppen: launchctl unload $PLIST"
echo "  Log: tail -f /tmp/vpn-router.log"
echo "  Browser: http://localhost:3080"
