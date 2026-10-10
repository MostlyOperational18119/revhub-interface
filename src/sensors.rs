//! Drivers for the I2C sensors the original REV Hub Interface supported.

use crate::hub::{Connection, Result};
use serde_json::{Value, json};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// One I2C device on one bus of one hub.
pub struct Device<'a> {
    conn: &'a mut Connection,
    hub: u8,
    bus: u8,
    addr: u8,
}

impl<'a> Device<'a> {
    pub fn new(conn: &'a mut Connection, hub: u8, bus: u8, addr: u8) -> Self {
        Self { conn, hub, bus, addr }
    }

    fn write(&mut self, data: &[u8]) -> Result<()> {
        self.conn.i2c_write(self.hub, self.bus, self.addr, data)
    }

    fn write_reg(&mut self, reg: u8, value: u8) -> Result<()> {
        self.write(&[reg, value])
    }

    fn read(&mut self, reg: u8, count: u8) -> Result<Vec<u8>> {
        self.conn.i2c_read_register(self.hub, self.bus, self.addr, reg, count)
    }

    fn read_u8(&mut self, reg: u8) -> Result<u8> {
        Ok(self.read(reg, 1)?.first().copied().unwrap_or(0))
    }

    fn read_u16_be(&mut self, reg: u8) -> Result<u16> {
        let b = self.read(reg, 2)?;
        Ok(u16::from_be_bytes([b.first().copied().unwrap_or(0), b.get(1).copied().unwrap_or(0)]))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorKind {
    Distance2m,
    ColorV3,
    ColorV2,
    Imu,
}

impl SensorKind {
    pub fn label(self) -> &'static str {
        match self {
            SensorKind::Distance2m => "REV 2m Distance Sensor",
            SensorKind::ColorV3 => "REV Color Sensor V3",
            SensorKind::ColorV2 => "REV Color Sensor V2",
            SensorKind::Imu => "BNO055 IMU",
        }
    }
}

/// Probes the bus for each supported sensor and initialises the first one found.
pub fn detect_and_init(conn: &mut Connection, hub: u8, bus: u8) -> Result<Option<SensorKind>> {
    if vl53l0x::detect(&mut Device::new(conn, hub, bus, vl53l0x::ADDRESS)) {
        vl53l0x::init(&mut Device::new(conn, hub, bus, vl53l0x::ADDRESS))?;
        return Ok(Some(SensorKind::Distance2m));
    }
    if color_v3::detect(&mut Device::new(conn, hub, bus, color_v3::ADDRESS)) {
        color_v3::init(&mut Device::new(conn, hub, bus, color_v3::ADDRESS))?;
        return Ok(Some(SensorKind::ColorV3));
    }
    if color_v2::detect(&mut Device::new(conn, hub, bus, color_v2::ADDRESS)) {
        color_v2::init(&mut Device::new(conn, hub, bus, color_v2::ADDRESS))?;
        return Ok(Some(SensorKind::ColorV2));
    }
    Ok(None)
}

pub fn read(conn: &mut Connection, hub: u8, bus: u8, kind: SensorKind) -> Result<Value> {
    match kind {
        SensorKind::Distance2m => vl53l0x::read(&mut Device::new(conn, hub, bus, vl53l0x::ADDRESS)),
        SensorKind::ColorV3 => color_v3::read(&mut Device::new(conn, hub, bus, color_v3::ADDRESS)),
        SensorKind::ColorV2 => color_v2::read(&mut Device::new(conn, hub, bus, color_v2::ADDRESS)),
        SensorKind::Imu => bno055::read(&mut Device::new(conn, hub, bus, bno055::ADDRESS)),
    }
}

fn color_json(r: u32, g: u32, b: u32, c: i64, prox: u32) -> Value {
    let max = r.max(g).max(b).max(1) as f64;
    let swatch = format!(
        "#{:02x}{:02x}{:02x}",
        (r as f64 / max * 255.0) as u8,
        (g as f64 / max * 255.0) as u8,
        (b as f64 / max * 255.0) as u8
    );
    json!({ "red": r, "green": g, "blue": b, "clear": c, "proximity": prox, "swatch": swatch })
}

/// REV Color Sensor V3 (Broadcom APDS-9151).
pub mod color_v3 {
    use super::*;

    pub const ADDRESS: u8 = 0x52;
    const PART_ID: u8 = 0x06;
    const EXPECTED_ID: u8 = 0xC2;
    const MAIN_CTRL: u8 = 0x00;
    const PS_PULSES: u8 = 0x02;
    const PS_MEAS_RATE: u8 = 0x03;
    const LS_GAIN: u8 = 0x05;
    const PS_DATA: u8 = 0x08;

    pub fn detect(dev: &mut Device) -> bool {
        dev.read_u8(PART_ID).is_ok_and(|id| id == EXPECTED_ID)
    }

    pub fn init(dev: &mut Device) -> Result<()> {
        dev.write_reg(MAIN_CTRL, 0x04 | 0x02 | 0x01)?; // RGB mode, light sensor + proximity enabled
        dev.write_reg(PS_PULSES, 32)?;
        dev.write_reg(PS_MEAS_RATE, 0x18 | 0x05)?; // 11-bit, 100 ms
        dev.write_reg(LS_GAIN, 0x03) // gain 9
    }

    pub fn read(dev: &mut Device) -> Result<Value> {
        // PS(2) IR(3) G(3) B(3) R(3), little endian, read as one block.
        let b = dev.read(PS_DATA, 14)?;
        if b.len() < 14 {
            return Ok(json!({ "error": "short read" }));
        }
        let u24 = |i: usize| (b[i] as u32 | (b[i + 1] as u32) << 8 | (b[i + 2] as u32) << 16) & 0x0F_FFFF;
        let prox = (b[0] as u32 | (b[1] as u32) << 8) & 0x07FF;
        let (ir, g, bl, r) = (u24(2), u24(5), u24(8), u24(11));
        let clear = r as i64 + g as i64 + bl as i64 - 2 * ir as i64;
        Ok(color_json(r, g, bl, clear, prox))
    }
}

/// REV Color Sensor V2 (AMS TMD37821).
pub mod color_v2 {
    use super::*;

    pub const ADDRESS: u8 = 0x39;
    const CMD: u8 = 0x80;
    const AUTO_INC: u8 = 0x20;
    const ENABLE: u8 = 0x00;
    const ATIME: u8 = 0x01;
    const PPULSE: u8 = 0x0E;
    const ID: u8 = 0x12;
    const CDATA: u8 = 0x14;
    const EXPECTED_ID: u8 = 0x60;

    pub fn detect(dev: &mut Device) -> bool {
        dev.read_u8(CMD | ID).is_ok_and(|id| id == EXPECTED_ID)
    }

    pub fn init(dev: &mut Device) -> Result<()> {
        dev.write_reg(CMD | ENABLE, 0x07)?; // power on, ALS + proximity enabled
        dev.write_reg(CMD | ATIME, 0xFF)?;
        dev.write_reg(CMD | PPULSE, 0x08)
    }

    pub fn read(dev: &mut Device) -> Result<Value> {
        // C, R, G, B, P as consecutive 16-bit little endian registers.
        let b = dev.read(CMD | AUTO_INC | CDATA, 10)?;
        if b.len() < 10 {
            return Ok(json!({ "error": "short read" }));
        }
        let w = |i: usize| b[i] as u32 | (b[i + 1] as u32) << 8;
        Ok(color_json(w(2), w(4), w(6), w(0) as i64, w(8)))
    }
}

/// Bosch BNO055 IMU built into older Expansion/Control Hubs (I2C bus 0).
pub mod bno055 {
    use super::*;

    pub const ADDRESS: u8 = 0x28;
    const CHIP_ID: u8 = 0x00;
    const EXPECTED_ID: u8 = 0xA0;
    const PAGE_ID: u8 = 0x07;
    const EUL_H_LSB: u8 = 0x1A;
    const GRV_DATA_X_LSB: u8 = 0x2E;
    const SYS_STAT: u8 = 0x39;
    const UNIT_SEL: u8 = 0x3B;
    const OPR_MODE: u8 = 0x3D;
    const PWR_MODE: u8 = 0x3E;
    const SYS_TRIGGER: u8 = 0x3F;
    const MODE_CONFIG: u8 = 0x00;
    const MODE_IMU: u8 = 0x08;

    pub fn detect(dev: &mut Device) -> bool {
        dev.read_u8(CHIP_ID).is_ok_and(|id| id == EXPECTED_ID)
    }

    /// Puts the IMU into IMU fusion mode. Returns whether fusion is running.
    pub fn init(dev: &mut Device) -> Result<bool> {
        for _ in 0..3 {
            dev.write_reg(OPR_MODE, MODE_CONFIG)?;
            sleep(Duration::from_millis(20));
            dev.write_reg(PWR_MODE, 0x00)?;
            dev.write_reg(SYS_TRIGGER, 0x80)?;
            dev.write_reg(PAGE_ID, 0)?;
            dev.write_reg(UNIT_SEL, 0x00)?; // m/s², degrees
            dev.write_reg(OPR_MODE, MODE_IMU)?;
            sleep(Duration::from_millis(20));
            if dev.read_u8(SYS_STAT)? == 5 {
                return Ok(true);
            }
            sleep(Duration::from_millis(100));
        }
        Ok(false)
    }

    fn vec3(dev: &mut Device, reg: u8) -> Result<[i16; 3]> {
        let b = dev.read(reg, 6)?;
        let v = |i: usize| i16::from_le_bytes([b.get(i).copied().unwrap_or(0), b.get(i + 1).copied().unwrap_or(0)]);
        Ok([v(0), v(2), v(4)])
    }

    pub fn read(dev: &mut Device) -> Result<Value> {
        let [h, r, p] = vec3(dev, EUL_H_LSB)?;
        let [gx, gy, gz] = vec3(dev, GRV_DATA_X_LSB)?;
        Ok(json!({
            "heading": h as f64 / 16.0,
            "roll": r as f64 / 16.0,
            "pitch": -(p as f64) / 16.0,
            "gravity": [gx as f64 / 100.0, gy as f64 / 100.0, gz as f64 / 100.0],
        }))
    }
}

/// REV 2m Distance Sensor (ST VL53L0X), following Pololu's reference driver.
pub mod vl53l0x {
    use super::*;

    pub const ADDRESS: u8 = 0x29;
    const SYSRANGE_START: u8 = 0x00;
    const SYSTEM_SEQUENCE_CONFIG: u8 = 0x01;
    const SYSTEM_INTERRUPT_CONFIG_GPIO: u8 = 0x0A;
    const SYSTEM_INTERRUPT_CLEAR: u8 = 0x0B;
    const RESULT_INTERRUPT_STATUS: u8 = 0x13;
    const RESULT_RANGE_STATUS: u8 = 0x14;
    const MSRC_CONFIG_CONTROL: u8 = 0x60;
    const FINAL_RANGE_CONFIG_MIN_COUNT_RATE_RTN_LIMIT: u8 = 0x44;
    const GPIO_HV_MUX_ACTIVE_HIGH: u8 = 0x84;
    const GLOBAL_CONFIG_SPAD_ENABLES_REF_0: u8 = 0xB0;
    const GLOBAL_CONFIG_REF_EN_START_SELECT: u8 = 0xB6;
    const DYNAMIC_SPAD_NUM_REQUESTED_REF_SPAD: u8 = 0x4E;
    const DYNAMIC_SPAD_REF_EN_START_OFFSET: u8 = 0x4F;
    const VHV_CONFIG_PAD_SCL_SDA_EXTSUP_HV: u8 = 0x89;

    /// ST's undocumented tuning settings, as (register, value) pairs.
    const TUNING: &[(u8, u8)] = &[
        (0xFF, 0x01), (0x00, 0x00), (0xFF, 0x00), (0x09, 0x00), (0x10, 0x00), (0x11, 0x00),
        (0x24, 0x01), (0x25, 0xFF), (0x75, 0x00), (0xFF, 0x01), (0x4E, 0x2C), (0x48, 0x00),
        (0x30, 0x20), (0xFF, 0x00), (0x30, 0x09), (0x54, 0x00), (0x31, 0x04), (0x32, 0x03),
        (0x40, 0x83), (0x46, 0x25), (0x60, 0x00), (0x27, 0x00), (0x50, 0x06), (0x51, 0x00),
        (0x52, 0x96), (0x56, 0x08), (0x57, 0x30), (0x61, 0x00), (0x62, 0x00), (0x64, 0x00),
        (0x65, 0x00), (0x66, 0xA0), (0xFF, 0x01), (0x22, 0x32), (0x47, 0x14), (0x49, 0xFF),
        (0x4A, 0x00), (0xFF, 0x00), (0x7A, 0x0A), (0x7B, 0x00), (0x78, 0x21), (0xFF, 0x01),
        (0x23, 0x34), (0x42, 0x00), (0x44, 0xFF), (0x45, 0x26), (0x46, 0x05), (0x40, 0x40),
        (0x0E, 0x06), (0x20, 0x1A), (0x43, 0x40), (0xFF, 0x00), (0x34, 0x03), (0x35, 0x44),
        (0xFF, 0x01), (0x31, 0x04), (0x4B, 0x09), (0x4C, 0x05), (0x4D, 0x04), (0xFF, 0x00),
        (0x44, 0x00), (0x45, 0x20), (0x47, 0x08), (0x48, 0x28), (0x67, 0x00), (0x70, 0x04),
        (0x71, 0x01), (0x72, 0xFE), (0x76, 0x00), (0x77, 0x00), (0xFF, 0x01), (0x0D, 0x01),
        (0xFF, 0x00), (0x80, 0x01), (0x01, 0xF8), (0xFF, 0x01), (0x8E, 0x01), (0x00, 0x01),
        (0xFF, 0x00), (0x80, 0x00),
    ];

    pub fn detect(dev: &mut Device) -> bool {
        let check = |dev: &mut Device, reg, expected| dev.read_u8(reg).is_ok_and(|v| v == expected);
        check(dev, 0xC0, 0xEE) && check(dev, 0xC1, 0xAA) && check(dev, 0xC2, 0x10)
    }

    fn write_seq(dev: &mut Device, seq: &[(u8, u8)]) -> Result<()> {
        seq.iter().try_for_each(|&(r, v)| dev.write_reg(r, v))
    }

    pub fn init(dev: &mut Device) -> Result<()> {
        // 2.8 V I/O mode.
        let v = dev.read_u8(VHV_CONFIG_PAD_SCL_SDA_EXTSUP_HV)?;
        dev.write_reg(VHV_CONFIG_PAD_SCL_SDA_EXTSUP_HV, v | 0x01)?;
        // Standard I2C mode, then fetch the stop variable.
        write_seq(dev, &[(0x88, 0x00), (0x80, 0x01), (0xFF, 0x01), (0x00, 0x00)])?;
        let stop_variable = dev.read_u8(0x91)?;
        write_seq(dev, &[(0x00, 0x01), (0xFF, 0x00), (0x80, 0x00)])?;
        // Disable SIGNAL_RATE_MSRC and SIGNAL_RATE_PRE_RANGE limit checks.
        let v = dev.read_u8(MSRC_CONFIG_CONTROL)?;
        dev.write_reg(MSRC_CONFIG_CONTROL, v | 0x12)?;
        // Signal rate limit 0.25 MCPS (Q9.7).
        dev.write(&[FINAL_RANGE_CONFIG_MIN_COUNT_RATE_RTN_LIMIT, 0x00, 0x20])?;
        dev.write_reg(SYSTEM_SEQUENCE_CONFIG, 0xFF)?;

        let (spad_count, aperture) = spad_info(dev)?;
        let mut spad_map: [u8; 6] = dev.read(GLOBAL_CONFIG_SPAD_ENABLES_REF_0, 6)?.try_into().unwrap_or([0; 6]);
        write_seq(dev, &[
            (0xFF, 0x01),
            (DYNAMIC_SPAD_REF_EN_START_OFFSET, 0x00),
            (DYNAMIC_SPAD_NUM_REQUESTED_REF_SPAD, 0x2C),
            (0xFF, 0x00),
            (GLOBAL_CONFIG_REF_EN_START_SELECT, 0xB4),
        ])?;
        let first = if aperture { 12 } else { 0 };
        let mut enabled = 0;
        for i in 0..48 {
            if i < first || enabled == spad_count {
                spad_map[i / 8] &= !(1 << (i % 8));
            } else if (spad_map[i / 8] >> (i % 8)) & 1 == 1 {
                enabled += 1;
            }
        }
        let mut block = vec![GLOBAL_CONFIG_SPAD_ENABLES_REF_0];
        block.extend_from_slice(&spad_map);
        dev.write(&block)?;

        write_seq(dev, TUNING)?;

        // Interrupt on new sample ready, active low.
        dev.write_reg(SYSTEM_INTERRUPT_CONFIG_GPIO, 0x04)?;
        let v = dev.read_u8(GPIO_HV_MUX_ACTIVE_HIGH)?;
        dev.write_reg(GPIO_HV_MUX_ACTIVE_HIGH, v & !0x10)?;
        dev.write_reg(SYSTEM_INTERRUPT_CLEAR, 0x01)?;
        dev.write_reg(SYSTEM_SEQUENCE_CONFIG, 0xE8)?;

        // Reference calibrations (VHV then phase).
        dev.write_reg(SYSTEM_SEQUENCE_CONFIG, 0x01)?;
        single_ref_calibration(dev, 0x40)?;
        dev.write_reg(SYSTEM_SEQUENCE_CONFIG, 0x02)?;
        single_ref_calibration(dev, 0x00)?;
        dev.write_reg(SYSTEM_SEQUENCE_CONFIG, 0xE8)?;

        // Start back-to-back continuous ranging.
        write_seq(dev, &[(0x80, 0x01), (0xFF, 0x01), (0x00, 0x00), (0x91, stop_variable), (0x00, 0x01), (0xFF, 0x00), (0x80, 0x00)])?;
        dev.write_reg(SYSRANGE_START, 0x02)
    }

    fn spad_info(dev: &mut Device) -> Result<(usize, bool)> {
        write_seq(dev, &[(0x80, 0x01), (0xFF, 0x01), (0x00, 0x00), (0xFF, 0x06)])?;
        let v = dev.read_u8(0x83)?;
        dev.write_reg(0x83, v | 0x04)?;
        write_seq(dev, &[(0xFF, 0x07), (0x81, 0x01), (0x80, 0x01), (0x94, 0x6B), (0x83, 0x00)])?;
        let deadline = Instant::now() + Duration::from_millis(200);
        while dev.read_u8(0x83)? == 0x00 && Instant::now() < deadline {}
        dev.write_reg(0x83, 0x01)?;
        let tmp = dev.read_u8(0x92)?;
        write_seq(dev, &[(0x81, 0x00), (0xFF, 0x06)])?;
        let v = dev.read_u8(0x83)?;
        dev.write_reg(0x83, v & !0x04)?;
        write_seq(dev, &[(0xFF, 0x01), (0x00, 0x01), (0xFF, 0x00), (0x80, 0x00)])?;
        Ok(((tmp & 0x7F) as usize, (tmp >> 7) & 1 == 1))
    }

    fn single_ref_calibration(dev: &mut Device, vhv_init: u8) -> Result<()> {
        dev.write_reg(SYSRANGE_START, 0x01 | vhv_init)?;
        let deadline = Instant::now() + Duration::from_millis(200);
        while dev.read_u8(RESULT_INTERRUPT_STATUS)? & 0x07 == 0 && Instant::now() < deadline {}
        dev.write_reg(SYSTEM_INTERRUPT_CLEAR, 0x01)?;
        dev.write_reg(SYSRANGE_START, 0x00)
    }

    pub fn read(dev: &mut Device) -> Result<Value> {
        let deadline = Instant::now() + Duration::from_millis(200);
        while dev.read_u8(RESULT_INTERRUPT_STATUS)? & 0x07 == 0 {
            if Instant::now() > deadline {
                return Ok(json!({ "error": "measurement timeout" }));
            }
        }
        let mm = dev.read_u16_be(RESULT_RANGE_STATUS + 10)?;
        dev.write_reg(SYSTEM_INTERRUPT_CLEAR, 0x01)?;
        // 8190/8191 mean "out of range".
        Ok(if mm >= 8190 { json!({ "distance_mm": Value::Null }) } else { json!({ "distance_mm": mm }) })
    }
}
