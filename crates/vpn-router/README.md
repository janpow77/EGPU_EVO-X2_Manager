# VPN Travel Router Manager

Rust-Backend + Web-UI + GTK-Tray-Widget zur Verwaltung eines **GL.iNet AXT1800** Travel-Routers mit NordVPN WireGuard.

Steuert den Router remote per SSH (nicht ueber die GL.iNet-API) -- laeuft auf dem **NUC** oder **MacBook**, nicht auf dem Router selbst.

## Ueberblick

```
vpn-router                   # Axum-Server (Port 3080)
vpn-router-widget            # GTK3 Tray-Icon (optional, nur Linux/NUC)
frontend/index.html          # Eingebettete Web-UI (SPA)
scripts/                     # Setup- und Deployment-Helfer
configs/                     # WireGuard-Konfigurationen (gitignored)
```

**Features:**
- VPN-Server wechseln (NordVPN WireGuard, deutsche Server)
- WiFi-Repeater scannen und verbinden (Hotel-WLAN als WAN-Quelle)
- WiFi Access Point konfigurieren (SSID/Passwort)
- Kill Switch + DNS-Leak-Schutz
- Traffic-Monitoring und IP-Geolocation
- Notfall-Wiederherstellung nach Factory Reset

## Voraussetzungen

| Tool | Zweck | Installation |
|---|---|---|
| Rust/Cargo | Build | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| sshpass | Passwort-Login (Notfall) | `brew install esolitos/ipa/sshpass` (macOS) / `apt install sshpass` (Linux) |
| SSH Key | Passwortloser Router-Zugriff | Siehe Abschnitt SSH-Key |

## Schnellstart

### macOS (MacBook)

```bash
# 1. Einmalig: Setup (Rust, sshpass, SSH-Key, Build)
cd crates/vpn-router
bash scripts/setup-macos.sh

# 2. Server starten
ROUTER_IP=192.168.8.1 cargo run --bin vpn-router

# 3. Browser oeffnen
open http://localhost:3080
```

### Linux (NUC)

```bash
# Build
cargo build --release --bin vpn-router

# Starten
ROUTER_IP=192.168.8.1 ./target/release/vpn-router

# Browser: http://localhost:3080
```

### GTK Tray-Widget (nur Linux/NUC)

```bash
cargo build --release --features widget --bin vpn-router-widget
./target/release/vpn-router-widget
```

Tray-Icon zeigt VPN-Status, Klick auf "Dashboard oeffnen" oeffnet die Web-UI.

## macOS Autostart (LaunchAgent)

```bash
# Binary bauen
cargo build --release --bin vpn-router

# LaunchAgent installieren (startet bei Login automatisch)
bash scripts/install-launchagent.sh

# Status pruefen
launchctl list | grep vpn-router

# Stoppen
launchctl unload ~/Library/LaunchAgents/de.vpn-router.plist

# Log
tail -f /tmp/vpn-router.log
```

## Konfiguration

### Umgebungsvariablen

| Variable | Default | Beschreibung |
|---|---|---|
| `ROUTER_IP` | `192.168.8.1` | IP des GL.iNet Routers |
| `ROUTER_PASSWORD` | *(leer)* | Fallback wenn SSH-Key fehlt |
| `BIND_ADDR` | `0.0.0.0:3080` | Server Bind-Adresse |

Alternativ in `.env` setzen (liegt im `crates/vpn-router/` Verzeichnis).

### SSH-Key einrichten

Der Server steuert den Router per SSH. Ohne Key wird `sshpass` + `ROUTER_PASSWORD` genutzt.

```bash
# Key generieren (einmalig)
ssh-keygen -t ed25519 -f ~/.ssh/glinet_key -N ''

# Auf Router installieren
ssh-copy-id -i ~/.ssh/glinet_key root@192.168.8.1

# Oder vom NUC kopieren (wenn dort bereits eingerichtet)
scp janpow@<NUC-IP>:~/.ssh/glinet_key* ~/.ssh/
chmod 600 ~/.ssh/glinet_key
```

## API-Endpunkte

| Methode | Pfad | Beschreibung |
|---|---|---|
| GET | `/` | Web-UI |
| POST | `/api/auth/login` | Router-Login (setzt SSH-Key) |
| GET | `/api/status` | Gesamtstatus (WAN, VPN, Traffic, System) |
| GET | `/api/board` | Router Hardware-Info |
| POST | `/api/wifi/scan` | WLANs in Reichweite scannen |
| POST | `/api/wifi/connect` | Mit WLAN verbinden (Repeater) |
| POST | `/api/wifi/disconnect` | Repeater trennen |
| GET | `/api/wifi/status` | Repeater-Status |
| GET | `/api/vpn/servers` | WireGuard-Server auflisten |
| POST | `/api/vpn/connect` | VPN-Server verbinden |
| POST | `/api/vpn/disconnect` | VPN trennen |
| POST | `/api/nordvpn/load-servers` | NordVPN-Server laden und deployen |
| POST | `/api/setup/wifi-ap` | WiFi Access Point konfigurieren |
| POST | `/api/setup/security` | Kill Switch + DNS einrichten |
| POST | `/api/setup/test` | Verbindungstest ausfuehren |
| POST | `/api/setup/init-password` | Router-Passwort setzen (nach Reset) |
| POST | `/api/setup/emergency-restore` | Komplett-Wiederherstellung |

## Web-UI Tabs

| Tab | Funktion |
|---|---|
| **Setup** | Ersteinrichtung: WiFi AP, NordVPN-Token, Sicherheit, Test |
| **WiFi** | Internet-Quelle: Ethernet/Starlink oder Hotel-WLAN als Repeater |
| **VPN** | Status, Server wechseln, Traffic-Anzeige, oeffentliche IP |
| **Notfall** | Factory-Reset-Recovery: Passwort, SSH, VPN, DNS, Firewall |

## Scripts

| Script | Zweck |
|---|---|
| `scripts/setup-macos.sh` | Einmal-Setup auf macOS (Rust, sshpass, SSH-Key, Build) |
| `scripts/install-launchagent.sh` | macOS Autostart einrichten |
| `scripts/configure-router.sh` | Router ueber GL.iNet-API konfigurieren (interaktiv) |
| `scripts/fetch-nordvpn-configs.sh` | NordVPN WireGuard-Configs herunterladen |
| `scripts/emergency-restore.sh` | Standalone Notfall-Restore (ohne Server) |
| `scripts/deploy.sh` | Web-UI direkt auf Router deployen (Legacy) |

## Architektur

```
MacBook/NUC                          GL.iNet AXT1800
+-----------------------+            +------------------+
| vpn-router (Port 3080)|---SSH----->| OpenWrt (ubus)   |
| - Axum REST API       |            | - WireGuard VPN  |
| - Embedded Web-UI     |            | - WiFi AP        |
| - NordVPN Client      |            | - Repeater       |
+-----------------------+            | - Firewall/DNS   |
        |                            +------------------+
        | HTTP (Browser)                     |
        v                                    | WireGuard
  [Browser auf                               v
   MacBook/Handy]                     [NordVPN Server]
```

Der Server kommuniziert **ausschliesslich per SSH** mit dem Router. Kein direkter Browser-Zugriff auf die Router-API noetig -- das vermeidet CORS-Probleme und die umstaendliche GL.iNet-Auth (Challenge/Hash).

## Troubleshooting

**Port 3080 belegt?**
```bash
ss -tlnp | grep 3080      # Linux
lsof -i :3080             # macOS
# Alternativen Port setzen:
BIND_ADDR=127.0.0.1:3090 cargo run --bin vpn-router
```

**SSH zum Router schlaegt fehl?**
```bash
# Manuell testen
ssh -i ~/.ssh/glinet_key -o ConnectTimeout=5 root@192.168.8.1 "echo OK"

# Fallback mit Passwort
ROUTER_PASSWORD=VpnRouter2024! cargo run --bin vpn-router
```

**Router nach Reset nicht erreichbar?**
1. Router Reset-Knopf 10 Sek druecken
2. 90 Sek warten (LEDs stabil)
3. WiFi "GL-AXT1800-xxx" verbinden (PW: siehe Router-Unterseite)
4. Web-UI Notfall-Tab oder: `bash scripts/emergency-restore.sh`
