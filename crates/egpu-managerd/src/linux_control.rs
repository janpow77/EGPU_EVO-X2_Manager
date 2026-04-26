use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use egpu_manager_common::error::{PcieError, ThunderboltError};
use egpu_manager_common::hal::{PcieControl, ThunderboltControl};
use tracing::{debug, info, warn};

use crate::driver_rebind;

const DEVICE_SETTLE_DELAY: Duration = Duration::from_secs(2);
const RESCAN_DEADLINE: Duration = Duration::from_secs(15);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

pub struct LinuxSysfsPcieControl {
    settle_delay: Duration,
    rescan_deadline: Duration,
}

impl Default for LinuxSysfsPcieControl {
    fn default() -> Self {
        Self {
            settle_delay: DEVICE_SETTLE_DELAY,
            rescan_deadline: RESCAN_DEADLINE,
        }
    }
}

pub struct LinuxSysfsThunderboltControl;

impl LinuxSysfsThunderboltControl {
    fn authorized_path(device_path: &str) -> PathBuf {
        Path::new("/sys/bus/thunderbolt/devices")
            .join(device_path)
            .join("authorized")
    }
}

#[async_trait]
impl ThunderboltControl for LinuxSysfsThunderboltControl {
    async fn deauthorize(&self, device_path: &str) -> Result<(), ThunderboltError> {
        let path = Self::authorized_path(device_path);
        write_tb_attr(&path, "0", device_path).await
    }

    async fn authorize(&self, device_path: &str) -> Result<(), ThunderboltError> {
        let path = Self::authorized_path(device_path);
        write_tb_attr(&path, "1", device_path).await
    }

    async fn is_authorized(&self, device_path: &str) -> Result<bool, ThunderboltError> {
        let path = Self::authorized_path(device_path);
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| ThunderboltError::DeviceError {
                device_path: device_path.to_string(),
                reason: format!("{}: {e}", path.display()),
            })?;
        Ok(content.trim() == "1")
    }
}

#[async_trait]
impl PcieControl for LinuxSysfsPcieControl {
    async fn function_level_reset(&self, pci_address: &str) -> Result<(), PcieError> {
        let device_path = pci_device_path(pci_address);
        if !device_path.exists() {
            return Err(PcieError::ResetFailed {
                pci_address: pci_address.to_string(),
                reason: "PCI-Device nicht vorhanden".to_string(),
            });
        }

        let upstream_resets = upstream_reset_paths(pci_address);
        let device_reset = device_path.join("reset");

        set_power_control_on(&device_path).await;

        if device_reset.exists() {
            info!("PCIe-Device-Reset über {}", device_reset.display());
            match write_pci_attr(&device_reset, "1", pci_address).await {
                Ok(()) => {
                    tokio::time::sleep(self.settle_delay).await;
                    if device_ready(pci_address) {
                        return Ok(());
                    }
                    warn!(
                        "Device-Reset fuer {} abgeschlossen, Device aber noch nicht bereit",
                        pci_address
                    );
                }
                Err(e) => {
                    warn!(
                        "Direkter Reset-Hook {} fehlgeschlagen: {} — versuche Upstream-Reset",
                        device_reset.display(),
                        e
                    );
                }
            }
        } else {
            warn!(
                "Kein direkter Reset-Hook fuer {} vorhanden, weiche auf Upstream-Reset aus",
                pci_address
            );
        }

        driver_rebind::set_driver_override(pci_address, "none").await;
        driver_rebind::block_companion_audio_autobind(pci_address).await;
        remove_companion_audio(pci_address).await;
        remove_device_if_present(pci_address).await;
        tokio::time::sleep(Duration::from_secs(1)).await;

        for reset_path in upstream_resets {
            if let Some(parent) = reset_path.parent() {
                set_power_control_on(parent).await;
            }
            info!("PCIe-Upstream-Reset über {}", reset_path.display());
            if let Err(e) = write_pci_attr(&reset_path, "1", pci_address).await {
                warn!(
                    "Upstream-Reset-Hook {} fehlgeschlagen: {}",
                    reset_path.display(),
                    e
                );
                continue;
            }
            tokio::time::sleep(self.settle_delay).await;
            trigger_global_rescan(pci_address).await?;
            if wait_for_device_ready(pci_address, self.rescan_deadline).await.is_ok() {
                return Ok(());
            }
        }

        trigger_global_rescan(pci_address).await?;
        wait_for_device_ready(pci_address, self.rescan_deadline).await
    }
}

fn pci_device_path(pci_address: &str) -> PathBuf {
    Path::new("/sys/bus/pci/devices").join(pci_address)
}

fn device_ready(pci_address: &str) -> bool {
    driver_rebind::is_pci_device_present(pci_address) && driver_rebind::is_device_responsive(pci_address)
}

async fn wait_for_device_ready(pci_address: &str, deadline: Duration) -> Result<(), PcieError> {
    let start = tokio::time::Instant::now();
    while start.elapsed() < deadline {
        if device_ready(pci_address) {
            return Ok(());
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }

    Err(PcieError::ResetFailed {
        pci_address: pci_address.to_string(),
        reason: format!(
            "Device nach Reset/Rescan nicht bereit nach {}s",
            deadline.as_secs()
        ),
    })
}

async fn trigger_global_rescan(pci_address: &str) -> Result<(), PcieError> {
    let rescan_path = Path::new("/sys/bus/pci/rescan");
    debug!("PCI-Rescan für {} über {}", pci_address, rescan_path.display());
    tokio::fs::write(rescan_path, "1")
        .await
        .map_err(|e| PcieError::ResetFailed {
            pci_address: pci_address.to_string(),
            reason: format!("{}: {e}", rescan_path.display()),
        })
}

async fn remove_companion_audio(pci_address: &str) {
    let Some(base) = pci_address.strip_suffix(".0") else {
        return;
    };
    let audio_address = format!("{base}.1");
    let audio_path = pci_device_path(&audio_address);
    if !audio_path.exists() {
        return;
    }

    let unbind_path = audio_path.join("driver/unbind");
    if unbind_path.exists()
        && let Err(e) = tokio::fs::write(&unbind_path, &audio_address).await
    {
        warn!(
            "Audio-Unbind fehlgeschlagen für {} über {}: {}",
            audio_address,
            unbind_path.display(),
            e
        );
    }

    let remove_path = audio_path.join("remove");
    if remove_path.exists()
        && let Err(e) = tokio::fs::write(&remove_path, "1").await
    {
        warn!(
            "Audio-Remove fehlgeschlagen für {} über {}: {}",
            audio_address,
            remove_path.display(),
            e
        );
    }
}

async fn remove_device_if_present(pci_address: &str) {
    let device_path = pci_device_path(pci_address);
    if !device_path.exists() {
        return;
    }

    let unbind_path = device_path.join("driver/unbind");
    if unbind_path.exists()
        && let Err(e) = tokio::fs::write(&unbind_path, pci_address).await
    {
        warn!(
            "Driver-Unbind fehlgeschlagen für {} über {}: {}",
            pci_address,
            unbind_path.display(),
            e
        );
    }

    let remove_path = device_path.join("remove");
    if remove_path.exists()
        && let Err(e) = tokio::fs::write(&remove_path, "1").await
    {
        warn!(
            "Device-Remove fehlgeschlagen für {} über {}: {}",
            pci_address,
            remove_path.display(),
            e
        );
    }
}

async fn set_power_control_on(device_path: &Path) {
    let power_control = device_path.join("power/control");
    if !power_control.exists() {
        return;
    }
    if let Err(e) = tokio::fs::write(&power_control, "on").await {
        warn!(
            "power/control=on fehlgeschlagen für {}: {}",
            power_control.display(),
            e
        );
    }
}

fn upstream_reset_paths(pci_address: &str) -> Vec<PathBuf> {
    let mut resets = Vec::new();
    let Ok(canonical) = std::fs::canonicalize(pci_device_path(pci_address)) else {
        return resets;
    };

    for ancestor in canonical.ancestors().skip(1) {
        let Some(name) = ancestor.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.contains(':') || !name.contains('.') {
            continue;
        }

        let subordinate_reset = ancestor.join("reset_subordinate");
        if subordinate_reset.exists() {
            resets.push(subordinate_reset);
            continue;
        }

        let reset_path = ancestor.join("reset");
        if reset_path.exists() {
            resets.push(reset_path);
        }
    }

    resets
}

async fn write_pci_attr(path: &Path, value: &str, pci_address: &str) -> Result<(), PcieError> {
    tokio::fs::write(path, value)
        .await
        .map_err(|e| PcieError::ResetFailed {
            pci_address: pci_address.to_string(),
            reason: format!("{}: {e}", path.display()),
        })
}

async fn write_tb_attr(path: &Path, value: &str, device_path: &str) -> Result<(), ThunderboltError> {
    tokio::fs::write(path, value)
        .await
        .map_err(|e| ThunderboltError::DeviceError {
            device_path: device_path.to_string(),
            reason: format!("{}: {e}", path.display()),
        })
}
