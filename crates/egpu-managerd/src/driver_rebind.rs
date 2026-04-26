//! Automatischer nvidia-Treiber-Rebind via sysfs.
//!
//! Löst das häufigste eGPU-Problem: GPU ist auf dem PCI-Bus sichtbar
//! (Thunderbolt-Link steht), aber der nvidia-Treiber hat sich nicht
//! gebunden → NVML/nvidia-smi sieht die GPU nicht.
//!
//! Häufigster Fall: NVRM "fallen off the bus" beim Boot — Config Space
//! liest nur 0xFF weil Thunderbolt-Link noch nicht trainiert war.
//! In diesem Fall muss das PCI-Device erst entfernt und der Bus
//! rescanned werden (einfaches nvidia/bind reicht nicht).
//!
//! Ablauf:
//!   1. Prüfe ob PCI-Device existiert (`/sys/bus/pci/devices/{pci}/vendor`)
//!   2. Prüfe ob Config Space erreichbar (vendor != 0xFFFF = "Ghost Device")
//!   3. Falls Ghost: PCI-Device removen + Bus rescannen
//!   4. Falls erreichbar aber nvidia nicht gebunden: nvidia/bind
//!   5. Falls bind fehlschlägt: PCI-Rescan als Fallback
//!
//! Voraussetzung: Daemon läuft mit CAP_SYS_ADMIN (siehe egpu-managerd.service).

use std::path::Path;
use std::time::Duration;

use tracing::{debug, error, info, warn};

/// Ergebnis eines Rebind-Versuchs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebindResult {
    /// Treiber war bereits gebunden — nichts zu tun.
    AlreadyBound,
    /// Treiber erfolgreich gebunden.
    Bound,
    /// Ghost-Device entfernt und Rescan getriggert — nächster Poll prüft Ergebnis.
    GhostRemoved,
    /// Bind fehlgeschlagen, PCI-Rescan als Fallback versucht.
    RescanTriggered,
    /// GPU nicht auf PCI-Bus gefunden (Kabel getrennt?).
    DeviceNotPresent,
    /// Rebind fehlgeschlagen.
    Failed(String),
}

const PCI_DRIVERS_AUTOPROBE_PATH: &str = "/sys/bus/pci/drivers_autoprobe";

pub fn read_pci_drivers_autoprobe() -> Option<bool> {
    std::fs::read_to_string(PCI_DRIVERS_AUTOPROBE_PATH)
        .ok()
        .map(|content| content.trim() != "0")
}

pub async fn set_pci_drivers_autoprobe(enabled: bool) -> std::io::Result<()> {
    tokio::fs::write(PCI_DRIVERS_AUTOPROBE_PATH, if enabled { "1" } else { "0" }).await
}

pub async fn set_driver_override(pci_address: &str, value: &str) {
    let override_path = format!("/sys/bus/pci/devices/{pci_address}/driver_override");
    if Path::new(&override_path).exists()
        && let Err(e) = tokio::fs::write(&override_path, value).await
    {
        warn!("driver_override={} fehlgeschlagen für {}: {}", value, pci_address, e);
    }
}

pub async fn block_companion_audio_autobind(pci_address: &str) {
    let Some(base) = pci_address.strip_suffix(".0") else {
        return;
    };
    let audio_address = format!("{base}.1");
    let audio_path = format!("/sys/bus/pci/devices/{audio_address}");
    if !Path::new(&audio_path).exists() {
        return;
    }

    let override_path = format!("{audio_path}/driver_override");
    if Path::new(&override_path).exists() {
        set_driver_override(&audio_address, "none").await;
    }

    let unbind_path = format!("{audio_path}/driver/unbind");
    if Path::new(&unbind_path).exists()
        && let Err(e) = tokio::fs::write(&unbind_path, &audio_address).await
    {
        warn!("Audio-Unbind fehlgeschlagen für {audio_address}: {e}");
    }
}

/// Prüft ob die eGPU auf dem PCI-Bus existiert.
pub fn is_pci_device_present(pci_address: &str) -> bool {
    Path::new(&format!("/sys/bus/pci/devices/{pci_address}/vendor")).exists()
}

/// Prüft ob das PCI-Device tatsächlich erreichbar ist (kein "Ghost").
/// Ein Ghost-Device hat vendor=0xFFFF weil der Config Space nur 0xFF liefert.
pub fn is_device_responsive(pci_address: &str) -> bool {
    let vendor_path = format!("/sys/bus/pci/devices/{pci_address}/vendor");
    match std::fs::read_to_string(&vendor_path) {
        Ok(vendor) => {
            let v = vendor.trim();
            // 0xffff = Device nicht erreichbar (Config Space = all 1s)
            v != "0xffff" && v != "0xFFFF" && !v.is_empty()
        }
        Err(_) => false,
    }
}

/// Prüft ob der Config Space nur 0xFF liefert (Ghost Device nach "fallen off the bus").
pub fn is_ghost_device(pci_address: &str) -> bool {
    is_pci_device_present(pci_address) && !is_device_responsive(pci_address)
}

/// Prüft ob der nvidia-Treiber an das PCI-Device gebunden ist.
pub fn is_nvidia_driver_bound(pci_address: &str) -> bool {
    let driver_link = format!("/sys/bus/pci/devices/{pci_address}/driver");
    match std::fs::read_link(&driver_link) {
        Ok(target) => {
            // driver symlink zeigt auf z.B. "../../../../bus/pci/drivers/nvidia"
            let target_str = target.to_string_lossy();
            target_str.ends_with("/nvidia")
        }
        Err(_) => false,
    }
}

/// Prüft ob die GPU einen Recovery-Eingriff braucht:
/// - Ghost Device (Config Space = 0xFF) → PCI-Remove + Rescan nötig
/// - Auf PCI-Bus vorhanden + erreichbar, aber nvidia nicht gebunden → Rebind nötig
pub fn needs_rebind(pci_address: &str) -> bool {
    if !is_pci_device_present(pci_address) {
        return false;
    }
    // Ghost Device ODER kein nvidia-Treiber
    is_ghost_device(pci_address) || !is_nvidia_driver_bound(pci_address)
}

/// Entfernt ein Ghost-PCI-Device und triggert einen Bus-Rescan.
///
/// Dies ist nötig wenn NVRM "fallen off the bus" gemeldet hat und der
/// Config Space nur 0xFF liefert. Ein einfaches nvidia/bind reicht dann nicht.
async fn remove_and_rescan(pci_address: &str) -> RebindResult {
    info!("Ghost-Device {pci_address} erkannt (Config Space = 0xFF) — entferne und rescanne");

    // Schritt 1: Audio-Function ebenfalls entfernen (05:00.1)
    block_companion_audio_autobind(pci_address).await;
    let audio_address = if pci_address.ends_with(".0") {
        Some(format!("{}.1", &pci_address[..pci_address.len() - 2]))
    } else {
        None
    };

    if let Some(ref audio) = audio_address {
        let audio_remove = format!("/sys/bus/pci/devices/{audio}/remove");
        if Path::new(&audio_remove).exists() {
            if let Err(e) = tokio::fs::write(&audio_remove, "1").await {
                warn!("Audio-Device {audio} remove fehlgeschlagen: {e}");
            } else {
                debug!("Audio-Device {audio} entfernt");
            }
        }
    }

    // Schritt 2: GPU-Device entfernen
    let remove_path = format!("/sys/bus/pci/devices/{pci_address}/remove");
    if let Err(e) = tokio::fs::write(&remove_path, "1").await {
        error!("PCI-Device {pci_address} remove fehlgeschlagen: {e}");
        return RebindResult::Failed(format!("remove fehlgeschlagen: {e}"));
    }

    // Warten bis Kernel den Remove verarbeitet hat
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Schritt 3: PCI-Bus rescannen
    if let Err(e) = tokio::fs::write("/sys/bus/pci/rescan", "1").await {
        error!("PCI-Rescan nach remove fehlgeschlagen: {e}");
        return RebindResult::Failed(format!("rescan nach remove fehlgeschlagen: {e}"));
    }

    info!("PCI-Rescan nach Ghost-Remove getriggert — warte auf Re-Enumeration");

    // Rescan + nvidia-Probe braucht Zeit
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Prüfen ob das Device jetzt erreichbar ist und nvidia gebunden
    if is_pci_device_present(pci_address) && is_device_responsive(pci_address) {
        if is_nvidia_driver_bound(pci_address) {
            info!("nvidia-Treiber nach Ghost-Remove+Rescan erfolgreich gebunden an {pci_address}");
            return RebindResult::Bound;
        }
        // Device ist da und erreichbar, aber nvidia hat sich nicht automatisch gebunden
        // → versuche manuelles Bind
        info!("Device {pci_address} nach Rescan erreichbar aber nvidia nicht gebunden — versuche Bind");
        return try_nvidia_bind(pci_address).await;
    }

    // Device ist nach Rescan noch nicht da — Thunderbolt-Link eventuell noch nicht trainiert
    warn!("Device {pci_address} nach Rescan nicht erreichbar — Thunderbolt-Link prüfen");
    RebindResult::GhostRemoved
}

/// Versucht nvidia direkt an ein erreichbares PCI-Device zu binden.
async fn try_nvidia_bind(pci_address: &str) -> RebindResult {
    prepare_device_for_bind(pci_address).await;
    block_companion_audio_autobind(pci_address).await;

    // Falls ein anderer Treiber gebunden ist, erst unbinden
    let driver_link = format!("/sys/bus/pci/devices/{pci_address}/driver");
    if Path::new(&driver_link).exists() && !is_nvidia_driver_bound(pci_address) {
        let unbind_path = format!("/sys/bus/pci/devices/{pci_address}/driver/unbind");
        info!("Anderer Treiber gebunden — unbinde zuerst über {unbind_path}");
        if let Err(e) = tokio::fs::write(&unbind_path, pci_address).await {
            warn!("Unbind fehlgeschlagen: {e} — versuche trotzdem nvidia bind");
        } else {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    // driver_override setzen
    let override_path = format!("/sys/bus/pci/devices/{pci_address}/driver_override");
    if Path::new(&override_path).exists() {
        set_driver_override(pci_address, "nvidia").await;
    }

    // Direktes Bind
    let bind_path = "/sys/bus/pci/drivers/nvidia/bind";
    match tokio::fs::write(bind_path, pci_address).await {
        Ok(()) => {
            tokio::time::sleep(Duration::from_secs(2)).await;

            if is_nvidia_driver_bound(pci_address) {
                info!("nvidia-Treiber erfolgreich an {pci_address} gebunden");
                let _ = tokio::fs::write(&override_path, "\0").await;
                return RebindResult::Bound;
            }
            warn!("nvidia bind geschrieben aber Treiber nicht gebunden");
        }
        Err(e) => {
            warn!("nvidia bind fehlgeschlagen: {e}");
        }
    }

    // Fallback: PCI-Rescan
    match tokio::fs::write("/sys/bus/pci/rescan", "1").await {
        Ok(()) => {
            info!("PCI-Bus-Rescan getriggert für {pci_address}");
            tokio::time::sleep(Duration::from_secs(3)).await;

            if is_nvidia_driver_bound(pci_address) {
                info!("nvidia-Treiber nach Rescan an {pci_address} gebunden");
                return RebindResult::Bound;
            }

            warn!("PCI-Rescan durchgeführt aber nvidia-Treiber immer noch nicht gebunden");
            RebindResult::RescanTriggered
        }
        Err(e) => {
            error!("PCI-Rescan fehlgeschlagen: {e}");
            RebindResult::Failed(format!("bind und rescan fehlgeschlagen: {e}"))
        }
    }
}

async fn prepare_device_for_bind(pci_address: &str) {
    let device_path = format!("/sys/bus/pci/devices/{pci_address}");

    let power_control = format!("{device_path}/power/control");
    if Path::new(&power_control).exists()
        && let Err(e) = tokio::fs::write(&power_control, "on").await
    {
        warn!("power/control=on fehlgeschlagen für {pci_address}: {e}");
    }

    let enable_path = format!("{device_path}/enable");
    if Path::new(&enable_path).exists() {
        match std::fs::read_to_string(&enable_path) {
            Ok(content) if content.trim() == "0" => {
                if let Err(e) = tokio::fs::write(&enable_path, "1").await {
                    warn!("enable=1 fehlgeschlagen für {pci_address}: {e}");
                } else {
                    debug!("PCI-Device {pci_address} via enable=1 aktiviert");
                }
            }
            Ok(_) => {}
            Err(e) => warn!("enable-State nicht lesbar für {pci_address}: {e}"),
        }
    }
}

/// Versucht die eGPU wiederherzustellen.
///
/// Strategie (zweistufig):
///   1. Ghost-Device (Config Space = 0xFF)? → PCI-Remove + Rescan
///   2. Device erreichbar aber nvidia nicht gebunden? → nvidia/bind + Rescan-Fallback
pub async fn try_rebind_nvidia(pci_address: &str) -> RebindResult {
    if !is_pci_device_present(pci_address) {
        debug!("eGPU {pci_address} nicht auf PCI-Bus — kein Rebind möglich");
        return RebindResult::DeviceNotPresent;
    }

    if is_nvidia_driver_bound(pci_address) {
        debug!("nvidia-Treiber bereits gebunden an {pci_address}");
        return RebindResult::AlreadyBound;
    }

    // Fall 1: Ghost-Device — Config Space = 0xFF, NVRM "fallen off the bus"
    if is_ghost_device(pci_address) {
        return remove_and_rescan(pci_address).await;
    }

    // Fall 2: Device erreichbar aber nvidia nicht gebunden
    info!("eGPU {pci_address} erreichbar aber nvidia nicht gebunden — starte Rebind");
    try_nvidia_bind(pci_address).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_needs_rebind_nonexistent_device() {
        assert!(!needs_rebind("9999:99:99.9"));
    }

    #[test]
    fn test_is_pci_device_present_nonexistent() {
        assert!(!is_pci_device_present("9999:99:99.9"));
    }

    #[test]
    fn test_is_nvidia_driver_bound_nonexistent() {
        assert!(!is_nvidia_driver_bound("9999:99:99.9"));
    }

    #[test]
    fn test_is_device_responsive_nonexistent() {
        assert!(!is_device_responsive("9999:99:99.9"));
    }

    #[test]
    fn test_is_ghost_device_nonexistent() {
        // Nicht existent ist kein Ghost (Ghost = existent + nicht erreichbar)
        assert!(!is_ghost_device("9999:99:99.9"));
    }

    #[test]
    fn test_rebind_result_equality() {
        assert_eq!(RebindResult::AlreadyBound, RebindResult::AlreadyBound);
        assert_eq!(RebindResult::Bound, RebindResult::Bound);
        assert_eq!(RebindResult::GhostRemoved, RebindResult::GhostRemoved);
        assert_ne!(RebindResult::Bound, RebindResult::AlreadyBound);
    }

    #[tokio::test]
    async fn test_try_rebind_nonexistent_device() {
        let result = try_rebind_nvidia("9999:99:99.9").await;
        assert_eq!(result, RebindResult::DeviceNotPresent);
    }
}
