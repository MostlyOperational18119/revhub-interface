//! High level access to REV Expansion Hubs over a serial link.

use crate::protocol::{BROADCAST_ADDRESS, Decoder, HOST_ADDRESS, Packet, RESPONSE_BIT, adc, cmd, nack};
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};
use thiserror::Error;

pub const BAUD_RATE: u32 = 460_800;
pub const FTDI_VID: u16 = 0x0403;
pub const FT230X_PID: u16 = 0x6015;
const RESPONSE_TIMEOUT: Duration = Duration::from_millis(250);
const RETRIES: usize = 3;

#[derive(Debug, Error)]
pub enum HubError {
    #[error("serial I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serial port error: {0}")]
    Serial(#[from] serialport::Error),
    #[error("no response from hub {0}")]
    Timeout(u8),
    #[error("hub {addr} rejected command 0x{cmd:04X}: {reason}")]
    Nack { addr: u8, cmd: u16, code: u8, reason: String },
    #[error("unexpected response 0x{got:04X} to command 0x{cmd:04X}")]
    Unexpected { cmd: u16, got: u16 },
    #[error("response payload too short for command 0x{0:04X}")]
    ShortPayload(u16),
}

impl HubError {
    pub fn nack_code(&self) -> Option<u8> {
        match self {
            HubError::Nack { code, .. } => Some(*code),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, HubError>;

/// Byte transport underneath the protocol: a real serial port or the simulator.
pub trait Transport: Send {
    fn write_all(&mut self, data: &[u8]) -> std::io::Result<()>;
    /// Reads whatever is available, waiting at most a short while. Returns 0 on timeout.
    fn read_some(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;
}

pub struct SerialTransport(Box<dyn serialport::SerialPort>);

impl SerialTransport {
    pub fn open(path: &str) -> Result<Self> {
        let port = serialport::new(path, BAUD_RATE)
            .data_bits(serialport::DataBits::Eight)
            .parity(serialport::Parity::None)
            .stop_bits(serialport::StopBits::One)
            .flow_control(serialport::FlowControl::None)
            .timeout(Duration::from_millis(5))
            .open()?;
        port.clear(serialport::ClearBuffer::All)?;
        Ok(Self(port))
    }
}

impl Transport for SerialTransport {
    fn write_all(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.0.write_all(data)?;
        self.0.flush()
    }

    fn read_some(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.0.read(buf) {
            Ok(n) => Ok(n),
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => Ok(0),
            Err(e) => Err(e),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PortInfo {
    pub path: String,
    pub serial: String,
}

/// Lists serial ports that look like REV hubs (FTDI devices with a `D...` serial number).
///
/// Reads sysfs directly rather than the udev database, which is not available inside
/// a Flatpak sandbox.
pub fn list_hub_ports() -> Vec<PortInfo> {
    list_hub_ports_in(Path::new("/sys/class/tty"))
}

fn list_hub_ports_in(class_dir: &Path) -> Vec<PortInfo> {
    let Ok(entries) = std::fs::read_dir(class_dir) else {
        return Vec::new();
    };
    let read = |dir: &Path, attr: &str| std::fs::read_to_string(dir.join(attr)).ok().map(|s| s.trim().to_string());
    let mut ports: Vec<PortInfo> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            // .../usb1/1-1/1-1:1.0/ttyUSB0: the USB device is the first ancestor with an idVendor.
            let device = std::fs::canonicalize(entry.path().join("device")).ok()?;
            let usb = device.ancestors().find(|p| p.join("idVendor").is_file())?;
            let vid = u16::from_str_radix(&read(usb, "idVendor")?, 16).ok()?;
            let serial = read(usb, "serial")?;
            (vid == FTDI_VID && serial.starts_with('D') && serial.len() > 2)
                .then(|| PortInfo { path: format!("/dev/{name}"), serial })
        })
        .collect();
    ports.sort_by(|a, b| a.path.cmp(&b.path));
    ports
}

#[derive(Debug, Clone)]
pub struct DiscoveredHub {
    pub address: u8,
    pub is_parent: bool,
}

#[derive(Debug, Clone, Default)]
pub struct BulkInput {
    pub digital_inputs: u8,
    pub encoders: [i32; 4],
    pub velocities: [i16; 4],
}

pub struct Connection {
    transport: Box<dyn Transport>,
    decoder: Decoder,
    msg_num: u8,
}

impl Connection {
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self { transport, decoder: Decoder::default(), msg_num: 0 }
    }

    fn next_msg_num(&mut self) -> u8 {
        self.msg_num = self.msg_num.wrapping_add(1);
        if self.msg_num == 0 {
            self.msg_num = 1;
        }
        self.msg_num
    }

    fn send(&mut self, dest: u8, packet_type: u16, payload: &[u8]) -> Result<u8> {
        let msg_num = self.next_msg_num();
        let packet = Packet { dest, src: HOST_ADDRESS, msg_num, ref_num: 0, packet_type, payload: payload.to_vec() };
        self.transport.write_all(&packet.encode())?;
        Ok(msg_num)
    }

    fn receive(&mut self, deadline: Instant) -> Result<Option<Packet>> {
        let mut buf = [0u8; 256];
        loop {
            if let Some(p) = self.decoder.next_packet() {
                return Ok(Some(p));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            let n = self.transport.read_some(&mut buf)?;
            self.decoder.push(&buf[..n]);
        }
    }

    /// Sends a command and waits for its response (ACK, NACK or `cmd | RESPONSE_BIT`).
    pub fn transact(&mut self, dest: u8, packet_type: u16, payload: &[u8]) -> Result<Vec<u8>> {
        for attempt in 0..RETRIES {
            let msg_num = self.send(dest, packet_type, payload)?;
            let deadline = Instant::now() + RESPONSE_TIMEOUT;
            while let Some(resp) = self.receive(deadline)? {
                if resp.ref_num != msg_num || resp.src != dest {
                    log::debug!("ignoring stale packet 0x{:04X} ref {}", resp.packet_type, resp.ref_num);
                    continue;
                }
                return match resp.packet_type {
                    t if t == cmd::ACK || t == (packet_type | RESPONSE_BIT) => Ok(resp.payload),
                    cmd::NACK => {
                        let code = resp.nack_code().unwrap_or(0);
                        Err(HubError::Nack { addr: dest, cmd: packet_type, code, reason: nack::describe(code) })
                    }
                    got => Err(HubError::Unexpected { cmd: packet_type, got }),
                };
            }
            log::debug!("timeout on 0x{packet_type:04X} to {dest} (attempt {})", attempt + 1);
        }
        Err(HubError::Timeout(dest))
    }

    /// Broadcasts a discovery request and collects every hub that answers.
    pub fn discover(&mut self) -> Result<Vec<DiscoveredHub>> {
        self.decoder.clear();
        self.send(BROADCAST_ADDRESS, cmd::DISCOVERY, &[])?;
        let mut hubs = Vec::new();
        // Daisy-chained (RS485) hubs answer later than the USB-attached one.
        let mut deadline = Instant::now() + Duration::from_millis(1000);
        while let Some(resp) = self.receive(deadline)? {
            if resp.packet_type == cmd::DISCOVERY | RESPONSE_BIT && !hubs.iter().any(|h: &DiscoveredHub| h.address == resp.src) {
                hubs.push(DiscoveredHub { address: resp.src, is_parent: resp.payload.first() == Some(&1) });
                deadline = deadline.max(Instant::now() + Duration::from_millis(300));
            }
        }
        hubs.sort_by_key(|h| (!h.is_parent, h.address));
        Ok(hubs)
    }

    // ---- module level -------------------------------------------------------------------------

    pub fn keep_alive(&mut self, addr: u8) -> Result<()> {
        self.transact(addr, cmd::KEEP_ALIVE, &[]).map(drop)
    }

    pub fn fail_safe(&mut self, addr: u8) -> Result<()> {
        self.transact(addr, cmd::FAIL_SAFE, &[]).map(drop)
    }

    /// Returns `(status_word, motor_alerts)`, clearing the latched status bits.
    pub fn module_status(&mut self, addr: u8) -> Result<(u8, u8)> {
        let p = self.transact(addr, cmd::GET_MODULE_STATUS, &[1])?;
        Ok((byte(&p, 0, cmd::GET_MODULE_STATUS)?, byte(&p, 1, cmd::GET_MODULE_STATUS)?))
    }

    pub fn set_module_address(&mut self, addr: u8, new_addr: u8) -> Result<()> {
        // The ACK may come from either address, or not at all, depending on firmware.
        match self.transact(addr, cmd::SET_NEW_MODULE_ADDRESS, &[new_addr]) {
            Ok(_) | Err(HubError::Timeout(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn version_string(&mut self, addr: u8) -> Result<String> {
        let p = self.transact(addr, cmd::READ_VERSION_STRING, &[])?;
        let len = byte(&p, 0, cmd::READ_VERSION_STRING)? as usize;
        let text = p.get(1..(1 + len).min(p.len())).unwrap_or_default();
        Ok(String::from_utf8_lossy(text).trim_end_matches('\0').trim().to_string())
    }

    pub fn bulk_input(&mut self, addr: u8) -> Result<BulkInput> {
        let p = self.transact(addr, cmd::GET_BULK_INPUT_DATA, &[])?;
        // Only the leading fields are decoded; their layout is shared by every firmware revision.
        if p.len() < 26 {
            return Err(HubError::ShortPayload(cmd::GET_BULK_INPUT_DATA));
        }
        let mut bulk = BulkInput { digital_inputs: p[0], ..Default::default() };
        for i in 0..4 {
            bulk.encoders[i] = i32::from_le_bytes(p[1 + 4 * i..5 + 4 * i].try_into().unwrap());
            bulk.velocities[i] = i16::from_le_bytes([p[18 + 2 * i], p[19 + 2 * i]]);
        }
        Ok(bulk)
    }

    /// Reads an ADC channel in engineering units (mV, mA or 0.1 °C depending on channel).
    pub fn adc(&mut self, addr: u8, channel: u8) -> Result<i16> {
        let p = self.transact(addr, cmd::GET_ADC, &[channel, 0])?;
        Ok(i16::from_le_bytes([byte(&p, 0, cmd::GET_ADC)?, byte(&p, 1, cmd::GET_ADC)?]))
    }

    // ---- motors ---------------------------------------------------------------------------------

    /// Mode 0 is constant power (open loop). `float_at_zero` selects coast vs brake.
    pub fn set_motor_mode(&mut self, addr: u8, motor: u8, mode: u8, float_at_zero: bool) -> Result<()> {
        self.transact(addr, cmd::SET_MOTOR_CHANNEL_MODE, &[motor, mode, float_at_zero as u8]).map(drop)
    }

    pub fn set_motor_enable(&mut self, addr: u8, motor: u8, enabled: bool) -> Result<()> {
        self.transact(addr, cmd::SET_MOTOR_CHANNEL_ENABLE, &[motor, enabled as u8]).map(drop)
    }

    /// `power` is in the range -32767..=32767.
    pub fn set_motor_power(&mut self, addr: u8, motor: u8, power: i16) -> Result<()> {
        let [lo, hi] = power.to_le_bytes();
        self.transact(addr, cmd::SET_MOTOR_CONSTANT_POWER, &[motor, lo, hi]).map(drop)
    }

    pub fn reset_encoder(&mut self, addr: u8, motor: u8) -> Result<()> {
        self.transact(addr, cmd::RESET_MOTOR_ENCODER, &[motor]).map(drop)
    }

    pub fn motor_current(&mut self, addr: u8, motor: u8) -> Result<i16> {
        self.adc(addr, adc::MOTOR_0_CURRENT + motor)
    }

    // ---- servos ---------------------------------------------------------------------------------

    pub fn set_servo_period(&mut self, addr: u8, servo: u8, period_us: u16) -> Result<()> {
        let [lo, hi] = period_us.to_le_bytes();
        self.transact(addr, cmd::SET_SERVO_CONFIGURATION, &[servo, lo, hi]).map(drop)
    }

    pub fn set_servo_pulse(&mut self, addr: u8, servo: u8, pulse_us: u16) -> Result<()> {
        let [lo, hi] = pulse_us.to_le_bytes();
        self.transact(addr, cmd::SET_SERVO_PULSE_WIDTH, &[servo, lo, hi]).map(drop)
    }

    pub fn set_servo_enable(&mut self, addr: u8, servo: u8, enabled: bool) -> Result<()> {
        self.transact(addr, cmd::SET_SERVO_ENABLE, &[servo, enabled as u8]).map(drop)
    }

    // ---- digital IO -----------------------------------------------------------------------------

    pub fn set_dio_direction(&mut self, addr: u8, pin: u8, output: bool) -> Result<()> {
        self.transact(addr, cmd::SET_DIO_DIRECTION, &[pin, output as u8]).map(drop)
    }

    pub fn set_dio_output(&mut self, addr: u8, pin: u8, value: bool) -> Result<()> {
        self.transact(addr, cmd::SET_SINGLE_DIO_OUTPUT, &[pin, value as u8]).map(drop)
    }

    // ---- I2C ------------------------------------------------------------------------------------

    /// Retries a command while the I2C engine reports it is busy with a previous transfer.
    fn i2c_command(&mut self, addr: u8, packet_type: u16, payload: &[u8]) -> Result<Vec<u8>> {
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            match self.transact(addr, packet_type, payload) {
                Err(e) if e.nack_code() == Some(nack::I2C_MASTER_BUSY) && Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                other => return other,
            }
        }
    }

    /// Polls a status query until the pending I2C transaction has finished.
    fn i2c_poll(&mut self, addr: u8, query: u16, bus: u8) -> Result<Vec<u8>> {
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            match self.transact(addr, query, &[bus]) {
                Err(e) if e.nack_code() == Some(nack::I2C_OPERATION_IN_PROGRESS) && Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                other => return other,
            }
        }
    }

    pub fn i2c_write(&mut self, addr: u8, bus: u8, device: u8, data: &[u8]) -> Result<()> {
        if let [single] = data {
            self.i2c_command(addr, cmd::I2C_WRITE_SINGLE_BYTE, &[bus, device, *single])?;
        } else {
            let mut payload = vec![bus, device, data.len() as u8];
            payload.extend_from_slice(data);
            self.i2c_command(addr, cmd::I2C_WRITE_MULTIPLE_BYTES, &payload)?;
        }
        match self.i2c_poll(addr, cmd::I2C_WRITE_STATUS_QUERY, bus) {
            Err(e) if e.nack_code() == Some(nack::I2C_NO_RESULTS_PENDING) => Ok(()),
            other => other.map(drop),
        }
    }

    pub fn i2c_read(&mut self, addr: u8, bus: u8, device: u8, count: u8) -> Result<Vec<u8>> {
        if count == 1 {
            self.i2c_command(addr, cmd::I2C_READ_SINGLE_BYTE, &[bus, device])?;
        } else {
            self.i2c_command(addr, cmd::I2C_READ_MULTIPLE_BYTES, &[bus, device, count])?;
        }
        let p = self.i2c_poll(addr, cmd::I2C_READ_STATUS_QUERY, bus)?;
        // Response: i2cStatus, bytesRead, data...
        let n = byte(&p, 1, cmd::I2C_READ_STATUS_QUERY)? as usize;
        let data = p.get(2..2 + n.min(count as usize)).ok_or(HubError::ShortPayload(cmd::I2C_READ_STATUS_QUERY))?;
        Ok(data.to_vec())
    }

    /// Writes the register pointer, then reads `count` bytes from it.
    pub fn i2c_read_register(&mut self, addr: u8, bus: u8, device: u8, register: u8, count: u8) -> Result<Vec<u8>> {
        self.i2c_write(addr, bus, device, &[register])?;
        self.i2c_read(addr, bus, device, count)
    }
}

fn byte(payload: &[u8], idx: usize, command: u16) -> Result<u8> {
    payload.get(idx).copied().ok_or(HubError::ShortPayload(command))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn finds_hubs_in_sysfs_layout() {
        let root = std::env::temp_dir().join(format!("revhub-sysfs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let class = root.join("class/tty");
        std::fs::create_dir_all(&class).unwrap();
        let add = |tty: &str, bus: &str, vid: &str, serial: &str| {
            let usb = root.join("devices").join(bus);
            let iface = usb.join(format!("{bus}:1.0"));
            std::fs::create_dir_all(iface.join(tty)).unwrap();
            std::fs::write(usb.join("idVendor"), format!("{vid}\n")).unwrap();
            std::fs::write(usb.join("serial"), format!("{serial}\n")).unwrap();
            std::fs::create_dir_all(class.join(tty)).unwrap();
            symlink(iface.join(tty), class.join(tty).join("device")).unwrap();
        };
        add("ttyUSB1", "1-2", "0403", "DQ1868AH");
        add("ttyUSB0", "1-1", "0403", "A9XYZ123"); // some other FTDI adapter
        add("ttyACM0", "1-3", "2341", "D1234567"); // not FTDI
        std::fs::create_dir_all(class.join("tty0")).unwrap(); // virtual console, no device link

        let ports = list_hub_ports_in(&class);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].path, "/dev/ttyUSB1");
        assert_eq!(ports[0].serial, "DQ1868AH");
    }
}
