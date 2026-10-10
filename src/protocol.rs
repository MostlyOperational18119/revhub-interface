//! REV Expansion Hub ("Lynx") serial packet protocol.
//!
//! Wire format (all multi-byte fields little endian):
//!
//! ```text
//! 'D' 'K' | len:u16 | dest:u8 | src:u8 | msg#:u8 | ref#:u8 | type:u16 | payload.. | cksum:u8
//! ```
//!
//! `len` is the total packet length including the frame bytes and checksum, and
//! the checksum is the 8-bit sum of every preceding byte.

pub const FRAME: [u8; 2] = [0x44, 0x4B];
pub const HEADER_LEN: usize = 10;
pub const MAX_PAYLOAD: usize = 512;
pub const HOST_ADDRESS: u8 = 0;
pub const BROADCAST_ADDRESS: u8 = 0xFF;
pub const RESPONSE_BIT: u16 = 0x8000;

#[allow(dead_code)]
pub mod cmd {
    pub const ACK: u16 = 0x7F01;
    pub const NACK: u16 = 0x7F02;
    pub const GET_MODULE_STATUS: u16 = 0x7F03;
    pub const KEEP_ALIVE: u16 = 0x7F04;
    pub const FAIL_SAFE: u16 = 0x7F05;
    pub const SET_NEW_MODULE_ADDRESS: u16 = 0x7F06;
    pub const QUERY_INTERFACE: u16 = 0x7F07;
    pub const SET_MODULE_LED_COLOR: u16 = 0x7F0A;
    pub const GET_MODULE_LED_COLOR: u16 = 0x7F0B;
    pub const DISCOVERY: u16 = 0x7F0F;

    const DEKA: u16 = 0x1000;
    pub const GET_BULK_INPUT_DATA: u16 = DEKA;
    pub const SET_SINGLE_DIO_OUTPUT: u16 = DEKA + 1;
    pub const SET_ALL_DIO_OUTPUTS: u16 = DEKA + 2;
    pub const SET_DIO_DIRECTION: u16 = DEKA + 3;
    pub const GET_DIO_DIRECTION: u16 = DEKA + 4;
    pub const GET_SINGLE_DIO_INPUT: u16 = DEKA + 5;
    pub const GET_ALL_DIO_INPUTS: u16 = DEKA + 6;
    pub const GET_ADC: u16 = DEKA + 7;
    pub const SET_MOTOR_CHANNEL_MODE: u16 = DEKA + 8;
    pub const GET_MOTOR_CHANNEL_MODE: u16 = DEKA + 9;
    pub const SET_MOTOR_CHANNEL_ENABLE: u16 = DEKA + 10;
    pub const GET_MOTOR_CHANNEL_ENABLE: u16 = DEKA + 11;
    pub const RESET_MOTOR_ENCODER: u16 = DEKA + 14;
    pub const SET_MOTOR_CONSTANT_POWER: u16 = DEKA + 15;
    pub const GET_MOTOR_ENCODER_POSITION: u16 = DEKA + 22;
    pub const SET_SERVO_CONFIGURATION: u16 = DEKA + 31;
    pub const SET_SERVO_PULSE_WIDTH: u16 = DEKA + 33;
    pub const SET_SERVO_ENABLE: u16 = DEKA + 35;
    pub const I2C_WRITE_SINGLE_BYTE: u16 = DEKA + 37;
    pub const I2C_WRITE_MULTIPLE_BYTES: u16 = DEKA + 38;
    pub const I2C_READ_SINGLE_BYTE: u16 = DEKA + 39;
    pub const I2C_READ_MULTIPLE_BYTES: u16 = DEKA + 40;
    pub const I2C_READ_STATUS_QUERY: u16 = DEKA + 41;
    pub const I2C_WRITE_STATUS_QUERY: u16 = DEKA + 42;
    pub const I2C_CONFIGURE_CHANNEL: u16 = DEKA + 43;
    pub const READ_VERSION_STRING: u16 = DEKA + 48;
}

/// ADC channels for `GET_ADC`.
#[allow(dead_code)]
pub mod adc {
    pub const ANALOG_0: u8 = 0;
    pub const GPIO_CURRENT: u8 = 4;
    pub const I2C_CURRENT: u8 = 5;
    pub const SERVO_CURRENT: u8 = 6;
    pub const BATTERY_CURRENT: u8 = 7;
    pub const MOTOR_0_CURRENT: u8 = 8;
    pub const MONITOR_5V: u8 = 12;
    pub const BATTERY_VOLTAGE: u8 = 13;
    pub const CONTROLLER_TEMPERATURE: u8 = 14;
}

/// NACK reason codes worth handling specially.
pub mod nack {
    pub const I2C_MASTER_BUSY: u8 = 40;
    pub const I2C_OPERATION_IN_PROGRESS: u8 = 41;
    pub const I2C_NO_RESULTS_PENDING: u8 = 42;

    pub fn describe(code: u8) -> String {
        match code {
            0..=9 => format!("parameter {code} out of range"),
            10..=17 => format!("DIO {} is not configured as an output", code - 10),
            20..=27 => format!("DIO {} is not configured as an input", code - 20),
            30 => "servo not configured before being enabled".into(),
            31 => "battery voltage too low to run servo".into(),
            40 => "I2C master busy".into(),
            41 => "I2C operation in progress".into(),
            42 => "no I2C results pending".into(),
            43 => "I2C query mismatch".into(),
            44 => "I2C timeout (SDA stuck)".into(),
            45 => "I2C timeout (SCK stuck)".into(),
            46 => "I2C timeout".into(),
            50 => "motor not configured before being enabled".into(),
            51 => "command invalid for motor mode".into(),
            52 => "battery voltage too low to run motor".into(),
            253 => "command implementation pending".into(),
            254 => "command routing error".into(),
            255 => "unknown packet type".into(),
            _ => format!("NACK code {code}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub dest: u8,
    pub src: u8,
    pub msg_num: u8,
    pub ref_num: u8,
    pub packet_type: u16,
    pub payload: Vec<u8>,
}

impl Packet {
    pub fn encode(&self) -> Vec<u8> {
        let len = (HEADER_LEN + self.payload.len() + 1) as u16;
        let mut out = Vec::with_capacity(len as usize);
        out.extend_from_slice(&FRAME);
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&[self.dest, self.src, self.msg_num, self.ref_num]);
        out.extend_from_slice(&self.packet_type.to_le_bytes());
        out.extend_from_slice(&self.payload);
        out.push(checksum(&out));
        out
    }

    pub fn is_nack(&self) -> bool {
        self.packet_type == cmd::NACK
    }

    pub fn nack_code(&self) -> Option<u8> {
        self.is_nack().then(|| self.payload.first().copied().unwrap_or(0))
    }
}

pub fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |acc, b| acc.wrapping_add(*b))
}

/// Incremental frame parser that tolerates garbage between packets.
#[derive(Default)]
pub struct Decoder {
    buf: Vec<u8>,
}

impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Returns the next complete, checksum-valid packet if one is buffered.
    pub fn next_packet(&mut self) -> Option<Packet> {
        loop {
            let start = self.buf.windows(2).position(|w| w == FRAME);
            match start {
                None => {
                    // Keep a trailing 'D' that may be the start of a frame.
                    let keep = usize::from(self.buf.last() == Some(&FRAME[0]));
                    self.buf.drain(..self.buf.len() - keep);
                    return None;
                }
                Some(s) => {
                    self.buf.drain(..s);
                }
            }
            if self.buf.len() < 4 {
                return None;
            }
            let len = u16::from_le_bytes([self.buf[2], self.buf[3]]) as usize;
            if !(HEADER_LEN + 1..=HEADER_LEN + 1 + MAX_PAYLOAD).contains(&len) {
                self.buf.drain(..1);
                continue;
            }
            if self.buf.len() < len {
                return None;
            }
            if checksum(&self.buf[..len - 1]) != self.buf[len - 1] {
                log::warn!("dropping packet with bad checksum");
                self.buf.drain(..1);
                continue;
            }
            let b = &self.buf;
            let packet = Packet {
                dest: b[4],
                src: b[5],
                msg_num: b[6],
                ref_num: b[7],
                packet_type: u16::from_le_bytes([b[8], b[9]]),
                payload: b[HEADER_LEN..len - 1].to_vec(),
            };
            self.buf.drain(..len);
            return Some(packet);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_alive_matches_reference_encoding() {
        // KeepAlive to module 2, msg 1: computed with the original Python implementation.
        let p = Packet { dest: 2, src: 0, msg_num: 1, ref_num: 0, packet_type: cmd::KEEP_ALIVE, payload: vec![] };
        let bytes = p.encode();
        assert_eq!(&bytes[..10], &[0x44, 0x4B, 0x0B, 0x00, 0x02, 0x00, 0x01, 0x00, 0x04, 0x7F]);
        assert_eq!(bytes[10], checksum(&bytes[..10]));
    }

    #[test]
    fn decoder_roundtrip_with_noise() {
        let p = Packet { dest: 0, src: 2, msg_num: 9, ref_num: 5, packet_type: 0x8000 | cmd::GET_ADC, payload: vec![0x34, 0x12] };
        let mut d = Decoder::default();
        d.push(&[0x00, 0x44, 0x13]);
        d.push(&p.encode()[..7]);
        assert_eq!(d.next_packet(), None);
        d.push(&p.encode()[7..]);
        assert_eq!(d.next_packet(), Some(p));
        assert_eq!(d.next_packet(), None);
    }

    #[test]
    fn decoder_rejects_bad_checksum() {
        let p = Packet { dest: 0, src: 2, msg_num: 1, ref_num: 1, packet_type: cmd::ACK, payload: vec![0] };
        let mut bytes = p.encode();
        *bytes.last_mut().unwrap() ^= 0xFF;
        let mut d = Decoder::default();
        d.push(&bytes);
        d.push(&p.encode());
        assert_eq!(d.next_packet(), Some(p));
    }
}
