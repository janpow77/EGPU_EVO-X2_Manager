#!/usr/bin/env bash
# evo-metrics auf EVO-X2 neustarten (nach Binary-Update)
ssh -t janpow@192.168.178.72 'sudo systemctl restart evo-metrics'
sleep 2
echo "--- Metrics Check ---"
curl -sf "http://192.168.178.72:8084/metrics" | python3 -c "
import sys, json
d = json.load(sys.stdin)
print(f'GTT:  {d[\"gtt\"][\"used_gb\"]:.1f} / {d[\"gtt\"][\"total_gb\"]:.1f} GB')
print(f'RAM:  {d[\"ram\"][\"used_gb\"]:.1f} / {d[\"ram\"][\"total_gb\"]:.1f} GB')
print(f'Services:')
for name, status in d['services'].items():
    print(f'  {name}: {status}')
"
