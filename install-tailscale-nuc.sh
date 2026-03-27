#!/usr/bin/env bash
# Tailscale auf dem NUC installieren
set -euo pipefail
curl -fsSL https://tailscale.com/install.sh | sh
sudo tailscale up
echo "---"
echo "Tailscale IP:"
tailscale ip -4
echo "---"
echo "Test EVO-X2 über Tailscale:"
curl -sf --connect-timeout 5 "http://100.81.4.99:11434/api/tags" | python3 -c "import sys,json; [print(m['name']) for m in json.load(sys.stdin)['models']]" || echo "NICHT ERREICHBAR"
