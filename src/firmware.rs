//! Expansion Hub firmware update.
//!
//! Replaces the Windows-only `sflash.exe` the original tool shelled out to:
//! 1. The FT230X CBUS pins (wired to the TM4C's reset/boot pins) are toggled through a
//!    libusb vendor request to drop the MCU into its ROM serial bootloader.
//! 2. The image is downloaded with TI's serial bootloader protocol at 230400 baud,
//!    to address 0 in 252-byte chunks (the same settings REV passed to sflash).

use crate::hub::{FT230X_PID, FTDI_VID};
use anyhow::{Context, Result, anyhow, bail, ensure};
use std::io::{Read, Write};
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

const BOOTLOADER_BAUD: u32 = 230_400;
const CHUNK_SIZE: usize = 252;
const MIN_IMAGE: u64 = 1000;
const MAX_IMAGE: u64 = 1 << 20;

const ACK: u8 = 0xCC;
const NAK: u8 = 0x33;
const COMMAND_PING: u8 = 0x20;
const COMMAND_DOWNLOAD: u8 = 0x21;
const COMMAND_GET_STATUS: u8 = 0x23;
const COMMAND_SEND_DATA: u8 = 0x24;
const COMMAND_RESET: u8 = 0x25;
const COMMAND_RET_SUCCESS: u8 = 0x40;

/// Validates a firmware image the same way the original tool did.
pub fn validate_image(path: &Path) -> Result<Vec<u8>> {
    ensure!(path.extension().is_some_and(|e| e.eq_ignore_ascii_case("bin")), "firmware files have a .bin extension");
    let len = std::fs::metadata(path)?.len();
    ensure!((MIN_IMAGE..=MAX_IMAGE).contains(&len), "invalid image size ({len} bytes); valid firmware is under 1 MB");
    let image = std::fs::read(path)?;
    // Sanity check the Cortex-M vector table: initial SP in SRAM, reset vector in flash (thumb).
    let word = |i: usize| u32::from_le_bytes(image[i..i + 4].try_into().unwrap());
    let (sp, reset) = (word(0), word(4));
    ensure!(
        (0x2000_0000..=0x2010_0000).contains(&sp) && reset & 1 == 1 && (reset as u64) < MAX_IMAGE,
        "file does not look like an Expansion Hub firmware image"
    );
    Ok(image)
}

/// Drives the FT230X CBUS bit-bang pins (bit 0 and bit 1).
struct Cbus {
    handle: rusb::DeviceHandle<rusb::GlobalContext>,
}

impl Cbus {
    fn open(serial: &str) -> Result<Self> {
        for device in rusb::devices().context("enumerating USB devices")?.iter() {
            let Ok(desc) = device.device_descriptor() else { continue };
            if desc.vendor_id() != FTDI_VID || desc.product_id() != FT230X_PID {
                continue;
            }
            let handle = device.open().map_err(|e| {
                anyhow!("cannot open FTDI device ({e}); install the udev rule from packaging/ to allow USB access")
            })?;
            let sn = handle.read_serial_number_string_ascii(&desc).unwrap_or_default();
            if serial.is_empty() || sn == serial {
                return Ok(Self { handle });
            }
        }
        bail!("no FT230X found with serial number {serial}")
    }

    fn set_bitmode(&self, mode: u8, mask: u8) -> Result<()> {
        const SIO_SET_BITMODE: u8 = 0x0B;
        let value = (mode as u16) << 8 | mask as u16;
        self.handle
            .write_control(0x40, SIO_SET_BITMODE, value, 1, &[], Duration::from_secs(1))
            .context("FTDI SET_BITMODE request failed")?;
        Ok(())
    }

    /// CBUS bit-bang mode: high nibble is the output-enable mask, low nibble the levels.
    fn write(&self, levels: u8) -> Result<()> {
        self.set_bitmode(0x20, 0x30 | (levels & 0x03))
    }

    fn release(&self) -> Result<()> {
        self.set_bitmode(0x00, 0x00)
    }
}

/// Checks that the hub's USB device can be opened (needed for firmware updates).
pub fn check_usb_access(serial: &str) -> Result<()> {
    Cbus::open(serial).map(drop)
}

struct Bootloader {
    port: Box<dyn serialport::SerialPort>,
}

impl Bootloader {
    fn open(path: &str) -> Result<Self> {
        let port = serialport::new(path, BOOTLOADER_BAUD).timeout(Duration::from_millis(50)).open()?;
        port.clear(serialport::ClearBuffer::All)?;
        Ok(Self { port })
    }

    fn read_byte(&mut self, deadline: Instant) -> Result<u8> {
        let mut b = [0u8];
        while Instant::now() < deadline {
            match self.port.read(&mut b) {
                Ok(1) => return Ok(b[0]),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => return Err(e.into()),
            }
        }
        bail!("bootloader did not respond")
    }

    /// Waits for ACK/NAK, skipping the zero padding the bootloader may emit.
    fn wait_ack(&mut self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.read_byte(deadline)? {
                0 => continue,
                ACK => return Ok(()),
                NAK => bail!("bootloader NAK"),
                other => log::debug!("bootloader: ignoring byte 0x{other:02X}"),
            }
        }
    }

    fn send_packet(&mut self, data: &[u8], timeout: Duration) -> Result<()> {
        let checksum = data.iter().fold(0u8, |a, b| a.wrapping_add(*b));
        let mut packet = vec![(data.len() + 2) as u8, checksum];
        packet.extend_from_slice(data);
        self.port.write_all(&packet)?;
        self.wait_ack(timeout)
    }

    fn status(&mut self) -> Result<u8> {
        self.send_packet(&[COMMAND_GET_STATUS], Duration::from_secs(2))?;
        let deadline = Instant::now() + Duration::from_secs(2);
        let size = loop {
            match self.read_byte(deadline)? {
                0 => continue,
                n => break n,
            }
        };
        let _checksum = self.read_byte(deadline)?;
        let mut body = Vec::new();
        for _ in 0..size.saturating_sub(2) {
            body.push(self.read_byte(deadline)?);
        }
        self.port.write_all(&[0, ACK])?;
        body.first().copied().ok_or_else(|| anyhow!("empty status response"))
    }

    fn command(&mut self, data: &[u8], timeout: Duration) -> Result<()> {
        self.send_packet(data, timeout)?;
        match self.status()? {
            COMMAND_RET_SUCCESS => Ok(()),
            s => bail!("bootloader reported error status 0x{s:02X} for command 0x{:02X}", data[0]),
        }
    }

    fn autobaud(&mut self) -> Result<()> {
        for _ in 0..5 {
            self.port.write_all(&[0x55, 0x55])?;
            if self.wait_ack(Duration::from_millis(500)).is_ok() {
                return Ok(());
            }
        }
        bail!("could not synchronise with the hub bootloader")
    }
}

/// Puts the hub into bootloader mode and writes `image`. `progress` gets (fraction, message).
pub fn flash(port_path: &str, ftdi_serial: &str, image: &[u8], progress: &mut dyn FnMut(f64, String)) -> Result<()> {
    progress(0.0, "Entering bootloader mode".into());
    let cbus = Cbus::open(ftdi_serial)?;
    for levels in [0, 1, 3] {
        cbus.write(levels)?;
        sleep(Duration::from_millis(100));
    }

    let result = (|| -> Result<()> {
        let mut bl = Bootloader::open(port_path)?;
        bl.autobaud()?;
        bl.send_packet(&[COMMAND_PING], Duration::from_secs(1))?;
        progress(0.02, "Bootloader connected, erasing flash".into());

        let mut padded = image.to_vec();
        padded.resize(image.len().div_ceil(4) * 4, 0xFF);
        let mut download = vec![COMMAND_DOWNLOAD];
        download.extend_from_slice(&0u32.to_be_bytes());
        download.extend_from_slice(&(padded.len() as u32).to_be_bytes());
        // The ROM erases every affected flash page before acknowledging.
        bl.command(&download, Duration::from_secs(20))?;

        let chunks = padded.chunks(CHUNK_SIZE).count();
        for (i, chunk) in padded.chunks(CHUNK_SIZE).enumerate() {
            let mut data = vec![COMMAND_SEND_DATA];
            data.extend_from_slice(chunk);
            bl.command(&data, Duration::from_secs(2)).with_context(|| format!("writing block {i}"))?;
            if i % 16 == 0 || i + 1 == chunks {
                let written = ((i + 1) * CHUNK_SIZE).min(padded.len());
                progress(0.05 + 0.93 * (i + 1) as f64 / chunks as f64, format!("Written {written} / {} bytes", padded.len()));
            }
        }

        // Release the CBUS pins before resetting so the MCU boots the new application.
        cbus.release()?;
        sleep(Duration::from_millis(50));
        let _ = bl.send_packet(&[COMMAND_RESET], Duration::from_millis(500));
        Ok(())
    })();

    if result.is_err() {
        let _ = cbus.release();
    }
    result?;
    progress(1.0, "Programming complete. The status LED should be blinking; reconnect to the hub.".into());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_firmware() {
        let dir = std::env::temp_dir().join(format!("revhub-fw-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("bad.bin");
        std::fs::write(&p, vec![0u8; 4096]).unwrap();
        assert!(validate_image(&p).is_err());
        let mut img = vec![0u8; 4096];
        img[..4].copy_from_slice(&0x2000_48ACu32.to_le_bytes());
        img[4..8].copy_from_slice(&0x0000_8EB1u32.to_le_bytes());
        std::fs::write(&p, &img).unwrap();
        assert!(validate_image(&p).is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
