//! A simulated pair of Expansion Hubs, used with `--simulate` to try the UI without hardware.

use crate::hub::Transport;
use crate::protocol::{Decoder, HOST_ADDRESS, Packet, RESPONSE_BIT, cmd};
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

#[derive(Default)]
struct SimHub {
    address: u8,
    parent: bool,
    motor_power: [i16; 4],
    motor_enabled: [bool; 4],
    encoder: [f64; 4],
    dio_output: u8,
    dio_values: u8,
    /// Register files keyed by (bus, device address), with the current register pointer.
    i2c: HashMap<(u8, u8), ([u8; 256], u8)>,
    i2c_pending: HashMap<u8, Vec<u8>>,
}

pub struct SimTransport {
    decoder: Decoder,
    outbox: VecDeque<u8>,
    hubs: Vec<SimHub>,
    start: Instant,
    last_tick: Instant,
}

impl SimTransport {
    pub fn new() -> Self {
        let mut hubs = vec![SimHub { address: 2, parent: true, ..Default::default() }, SimHub { address: 3, ..Default::default() }];
        for hub in &mut hubs {
            let mut imu = [0u8; 256];
            imu[0x00] = 0xA0;
            imu[0x39] = 5;
            hub.i2c.insert((0, 0x28), (imu, 0));
        }
        let mut color = [0u8; 256];
        color[0x06] = 0xC2;
        hubs[0].i2c.insert((1, 0x52), (color, 0));
        let mut tof = [0u8; 256];
        tof[0xC0] = 0xEE;
        tof[0xC1] = 0xAA;
        tof[0xC2] = 0x10;
        tof[0x13] = 0x07;
        tof[0x83] = 0x01;
        tof[0x92] = 0x85;
        hubs[0].i2c.insert((2, 0x29), (tof, 0));
        Self { decoder: Decoder::default(), outbox: VecDeque::new(), hubs, start: Instant::now(), last_tick: Instant::now() }
    }

    fn tick(&mut self) {
        let dt = self.last_tick.elapsed().as_secs_f64();
        self.last_tick = Instant::now();
        let t = self.start.elapsed().as_secs_f64();
        for hub in &mut self.hubs {
            for m in 0..4 {
                if hub.motor_enabled[m] {
                    // ~2800 counts/s at full power, like a REV HD Hex motor.
                    hub.encoder[m] += hub.motor_power[m] as f64 / 32767.0 * 2800.0 * dt;
                }
            }
            hub.dio_values = (hub.dio_output & 0x0F) | (((t * 2.0) as u8 & 1) << 7);
            if let Some((regs, _)) = hub.i2c.get_mut(&(0, 0x28)) {
                let heading = ((t * 20.0) % 360.0 * 16.0) as i16;
                let roll = ((t * 0.7).sin() * 10.0 * 16.0) as i16;
                regs[0x1A..0x1C].copy_from_slice(&heading.to_le_bytes());
                regs[0x1C..0x1E].copy_from_slice(&roll.to_le_bytes());
                regs[0x2E..0x30].copy_from_slice(&((roll as f64 / 16.0 * 1.7) as i16).to_le_bytes());
                regs[0x32..0x34].copy_from_slice(&981i16.to_le_bytes());
            }
            if let Some((regs, _)) = hub.i2c.get_mut(&(1, 0x52)) {
                let phase = t * 0.5;
                let put = |regs: &mut [u8; 256], at: usize, v: u32| regs[at..at + 3].copy_from_slice(&v.to_le_bytes()[..3]);
                put(regs, 0x13, (600.0 + 500.0 * phase.sin()) as u32);
                put(regs, 0x0D, (600.0 + 500.0 * (phase + 2.1).sin()) as u32);
                put(regs, 0x10, (600.0 + 500.0 * (phase + 4.2).sin()) as u32);
                put(regs, 0x0A, 80);
                regs[0x08..0x0A].copy_from_slice(&((300.0 + 250.0 * (t * 1.3).sin()) as u16).to_le_bytes());
            }
            if let Some((regs, _)) = hub.i2c.get_mut(&(2, 0x29)) {
                let mm = (400.0 + 300.0 * (t * 0.8).sin()) as u16;
                regs[0x1E..0x20].copy_from_slice(&mm.to_be_bytes());
            }
        }
    }

    fn respond(&mut self, req: &Packet) {
        self.tick();
        let t = self.start.elapsed().as_secs_f64();
        let targets: Vec<usize> = if req.packet_type == cmd::DISCOVERY {
            (0..self.hubs.len()).collect()
        } else {
            self.hubs.iter().position(|h| h.address == req.dest).into_iter().collect()
        };
        for idx in targets {
            let hub = &mut self.hubs[idx];
            let p = &req.payload;
            let arg = |i: usize| p.get(i).copied().unwrap_or(0);
            let reply: Option<Vec<u8>> = match req.packet_type {
                cmd::DISCOVERY => Some(vec![hub.parent as u8]),
                cmd::GET_MODULE_STATUS => Some(vec![0, 0]),
                cmd::READ_VERSION_STRING => {
                    let s = b"HW: 20, Maj: 1, Min: 8, Eng: 2 (simulated)";
                    let mut v = vec![s.len() as u8];
                    v.extend_from_slice(s);
                    Some(v)
                }
                cmd::GET_ADC => {
                    let ch = arg(0);
                    let v: i16 = match ch {
                        0..=3 => (1650.0 + 1600.0 * (t * 0.3 + ch as f64).sin()) as i16,
                        8..=11 => (hub.motor_power[(ch - 8) as usize].unsigned_abs() as f64 / 32767.0 * 1800.0) as i16,
                        12 => 5020,
                        13 => (12_600.0 - 400.0 * hub.motor_power.iter().map(|p| p.unsigned_abs() as f64 / 32767.0).sum::<f64>()) as i16,
                        14 => 385,
                        _ => 0,
                    };
                    Some(v.to_le_bytes().to_vec())
                }
                cmd::GET_BULK_INPUT_DATA => {
                    let mut v = vec![hub.dio_values];
                    for e in hub.encoder {
                        v.extend_from_slice(&(e as i32).to_le_bytes());
                    }
                    v.push(0);
                    for pw in hub.motor_power {
                        v.extend_from_slice(&((pw as f64 / 32767.0 * 2800.0) as i16).to_le_bytes());
                    }
                    v.extend_from_slice(&[0; 8]);
                    Some(v)
                }
                cmd::SET_MOTOR_CONSTANT_POWER => {
                    hub.motor_power[arg(0) as usize % 4] = i16::from_le_bytes([arg(1), arg(2)]);
                    None
                }
                cmd::SET_MOTOR_CHANNEL_ENABLE => {
                    hub.motor_enabled[arg(0) as usize % 4] = arg(1) != 0;
                    None
                }
                cmd::RESET_MOTOR_ENCODER => {
                    hub.encoder[arg(0) as usize % 4] = 0.0;
                    None
                }
                cmd::SET_SINGLE_DIO_OUTPUT => {
                    let bit = 1 << (arg(0) % 8);
                    if arg(1) != 0 { hub.dio_output |= bit } else { hub.dio_output &= !bit }
                    None
                }
                cmd::FAIL_SAFE => {
                    hub.motor_enabled = [false; 4];
                    hub.motor_power = [0; 4];
                    None
                }
                cmd::I2C_WRITE_SINGLE_BYTE | cmd::I2C_WRITE_MULTIPLE_BYTES => {
                    let (bus, dev) = (arg(0), arg(1));
                    let data: Vec<u8> =
                        if req.packet_type == cmd::I2C_WRITE_SINGLE_BYTE { vec![arg(2)] } else { p[3..].to_vec() };
                    if let Some((regs, ptr)) = hub.i2c.get_mut(&(bus, dev)) {
                        // The Color Sensor V2 command byte carries flag bits above the register number.
                        *ptr = if dev == 0x39 { data[0] & 0x1F } else { data[0] };
                        for (i, b) in data[1..].iter().enumerate() {
                            // Leave simulated read-only status registers alone.
                            let r = ptr.wrapping_add(i as u8);
                            if !matches!(r, 0x13 | 0x83 | 0x92) {
                                regs[r as usize] = *b;
                            }
                        }
                    }
                    None
                }
                cmd::I2C_WRITE_STATUS_QUERY => Some(vec![0, 0]),
                cmd::I2C_READ_SINGLE_BYTE | cmd::I2C_READ_MULTIPLE_BYTES => {
                    let (bus, dev) = (arg(0), arg(1));
                    let n = if req.packet_type == cmd::I2C_READ_SINGLE_BYTE { 1 } else { arg(2) as usize };
                    let data = match hub.i2c.get(&(bus, dev)) {
                        Some((regs, ptr)) => (0..n).map(|i| regs[(*ptr as usize + i) % 256]).collect(),
                        None => vec![0xFF; n],
                    };
                    hub.i2c_pending.insert(bus, data);
                    None
                }
                cmd::I2C_READ_STATUS_QUERY => {
                    let data = hub.i2c_pending.remove(&arg(0)).unwrap_or_default();
                    let mut v = vec![0, data.len() as u8];
                    v.extend(data);
                    Some(v)
                }
                _ => None,
            };
            let (packet_type, payload) = match reply {
                Some(payload) => (req.packet_type | RESPONSE_BIT, payload),
                None => (cmd::ACK, vec![0]),
            };
            let resp = Packet { dest: HOST_ADDRESS, src: hub.address, msg_num: 0, ref_num: req.msg_num, packet_type, payload };
            self.outbox.extend(resp.encode());
        }
        if req.packet_type == cmd::SET_NEW_MODULE_ADDRESS {
            if let Some(h) = self.hubs.iter_mut().find(|h| h.address == req.dest) {
                h.address = req.payload[0];
            }
        }
    }
}

impl Transport for SimTransport {
    fn write_all(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.decoder.push(data);
        while let Some(p) = self.decoder.next_packet() {
            self.respond(&p);
        }
        Ok(())
    }

    fn read_some(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.outbox.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(1));
            return Ok(0);
        }
        let n = buf.len().min(self.outbox.len());
        for (dst, src) in buf.iter_mut().zip(self.outbox.drain(..n)) {
            *dst = src;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::Connection;
    use crate::sensors::{self, SensorKind};

    #[test]
    fn end_to_end_against_simulator() {
        let mut conn = Connection::new(Box::new(SimTransport::new()));
        let hubs = conn.discover().unwrap();
        assert_eq!(hubs.len(), 2);
        assert!(hubs[0].is_parent && hubs[0].address == 2);
        assert!(conn.version_string(2).unwrap().starts_with("HW: 20"));

        conn.set_motor_power(2, 1, 16000).unwrap();
        conn.set_motor_enable(2, 1, true).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        let bulk = conn.bulk_input(2).unwrap();
        assert!(bulk.encoders[1] > 0 && bulk.velocities[1] > 0);
        assert!(conn.motor_current(2, 1).unwrap() > 0);

        assert_eq!(sensors::detect_and_init(&mut conn, 2, 0).unwrap(), None);
        assert_eq!(sensors::detect_and_init(&mut conn, 2, 1).unwrap(), Some(SensorKind::ColorV3));
        assert_eq!(sensors::detect_and_init(&mut conn, 2, 2).unwrap(), Some(SensorKind::Distance2m));
        let d = sensors::read(&mut conn, 2, 2, SensorKind::Distance2m).unwrap();
        let mm = d["distance_mm"].as_u64().unwrap();
        assert!((100..=700).contains(&mm), "{d}");
        let c = sensors::read(&mut conn, 2, 1, SensorKind::ColorV3).unwrap();
        assert!(c["red"].as_u64().unwrap() > 0 && c["swatch"].as_str().unwrap().starts_with('#'), "{c}");

        let mut imu = sensors::Device::new(&mut conn, 3, 0, sensors::bno055::ADDRESS);
        assert!(sensors::bno055::detect(&mut imu));
        assert!(sensors::bno055::init(&mut imu).unwrap());
        let r = sensors::read(&mut conn, 3, 0, SensorKind::Imu).unwrap();
        assert!(r["gravity"][2].as_f64().unwrap() > 9.0, "{r}");

        conn.set_module_address(3, 7).unwrap();
        conn.keep_alive(7).unwrap();
        assert!(matches!(conn.keep_alive(3), Err(crate::hub::HubError::Timeout(3))));
    }
}
