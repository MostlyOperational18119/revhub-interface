//! Background thread that owns the hub connection.
//!
//! The UI sends [`Command`]s; the worker applies them, polls the hubs every
//! [`POLL_PERIOD`] (which also keeps the hubs' 2.5 s fail-safe watchdog fed) and
//! publishes [`Event`]s back to the Qt thread.

use crate::firmware;
use crate::hub::{self, Connection, HubError, SerialTransport};
use crate::protocol::adc;
use crate::sensors::{self, SensorKind};
use crate::sim::SimTransport;
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const POLL_PERIOD: Duration = Duration::from_millis(100);
const SLOW_POLL_EVERY: u32 = 10;
const MAX_FAILED_POLLS: u32 = 5;
pub const SERVO_PERIOD_US: u16 = 20_000;

#[derive(Debug)]
pub enum Command {
    Connect,
    Disconnect,
    EmergencyStop,
    SetMotorPower { hub: usize, motor: u8, power: i16 },
    SetMotorFloat { hub: usize, motor: u8, float: bool },
    ResetEncoder { hub: usize, motor: u8 },
    SetServo { hub: usize, servo: u8, pulse_us: u16, enabled: bool },
    SetDioDirection { hub: usize, pin: u8, output: bool },
    SetDioOutput { hub: usize, pin: u8, value: bool },
    I2cDetect { hub: usize, bus: u8 },
    I2cPoll { hub: usize, bus: u8, enabled: bool },
    ImuInit { hub: usize },
    ImuPoll { hub: usize, enabled: bool },
    SetAddress { hub: usize, address: u8 },
    Flash { path: PathBuf },
    Shutdown,
}

impl Command {
    /// Commands with the same key supersede each other when they queue up (e.g. slider drags).
    fn coalesce_key(&self) -> Option<(u8, usize, u8)> {
        match *self {
            Command::SetMotorPower { hub, motor, .. } => Some((0, hub, motor)),
            Command::SetServo { hub, servo, .. } => Some((1, hub, servo)),
            _ => None,
        }
    }
}

pub enum Event {
    State(Value),
    Message { text: String, error: bool },
    FlashProgress { fraction: f64, message: String },
}

#[derive(Default, Clone)]
struct Motor {
    power: i16,
    enabled: bool,
    float: bool,
    position: i32,
    velocity: i16,
    current_ma: i16,
}

#[derive(Default, Clone)]
struct Servo {
    pulse_us: u16,
    enabled: bool,
}

#[derive(Default, Clone)]
struct Dio {
    output: bool,
    value: bool,
}

#[derive(Default, Clone)]
struct I2cSlot {
    sensor: Option<SensorKind>,
    polling: bool,
    reading: Option<Value>,
    message: String,
}

#[derive(Default)]
struct HubState {
    address: u8,
    parent: bool,
    firmware: String,
    battery_mv: i16,
    monitor_5v_mv: i16,
    temperature_dc: i16,
    status: u8,
    motor_alerts: u8,
    motors: [Motor; 4],
    servos: [Servo; 6],
    dio: [Dio; 8],
    analog_mv: [i16; 4],
    i2c: [I2cSlot; 4],
    imu: I2cSlot,
}

impl HubState {
    fn new(address: u8, parent: bool) -> Self {
        let mut h = Self { address, parent, ..Default::default() };
        for m in &mut h.motors {
            m.float = true;
        }
        for s in &mut h.servos {
            s.pulse_us = 1500;
        }
        h
    }

    fn alerts(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (bit, text) in [(2, "Fail-safe active"), (3, "Controller over temperature"), (4, "Battery low"), (5, "RS485/HIB fault")] {
            if self.status & (1 << bit) != 0 {
                out.push(text.to_string());
            }
        }
        for m in 0..4 {
            if self.motor_alerts & (1 << (m + 4)) != 0 {
                out.push(format!("Motor {m} H-bridge over temperature"));
            }
        }
        out
    }

    fn to_json(&self) -> Value {
        let slot = |s: &I2cSlot| {
            json!({
                "device": s.sensor.map(SensorKind::label),
                "kind": s.sensor.map(|k| format!("{k:?}")),
                "polling": s.polling,
                "reading": s.reading.clone(),
                "message": s.message,
            })
        };
        json!({
            "address": self.address,
            "parent": self.parent,
            "firmware": self.firmware,
            "battery": self.battery_mv as f64 / 1000.0,
            "monitor5v": self.monitor_5v_mv as f64 / 1000.0,
            "temperature": self.temperature_dc as f64 / 10.0,
            "alerts": self.alerts(),
            "motors": self.motors.iter().map(|m| json!({
                "power": m.power, "enabled": m.enabled, "float": m.float,
                "position": m.position, "velocity": m.velocity, "current": m.current_ma,
            })).collect::<Vec<_>>(),
            "servos": self.servos.iter().map(|s| json!({ "pulse": s.pulse_us, "enabled": s.enabled })).collect::<Vec<_>>(),
            "dio": self.dio.iter().map(|d| json!({ "output": d.output, "value": d.value })).collect::<Vec<_>>(),
            "analog": self.analog_mv.iter().map(|mv| *mv as f64 / 1000.0).collect::<Vec<_>>(),
            "i2c": self.i2c.iter().map(slot).collect::<Vec<_>>(),
            "imu": slot(&self.imu),
        })
    }
}

pub struct Worker {
    simulate: bool,
    conn: Option<Connection>,
    port: Option<hub::PortInfo>,
    hubs: Vec<HubState>,
    failed_polls: u32,
    tick: u32,
    flash: Option<(f64, String)>,
    emit: Box<dyn FnMut(Event) + Send>,
}

impl Worker {
    pub fn spawn(simulate: bool, emit: Box<dyn FnMut(Event) + Send>) -> Sender<Command> {
        let (tx, rx) = crossbeam_channel::unbounded();
        std::thread::Builder::new()
            .name("hub-worker".into())
            .spawn(move || {
                let mut w = Worker { simulate, conn: None, port: None, hubs: Vec::new(), failed_polls: 0, tick: 0, flash: None, emit };
                if simulate {
                    w.handle(Command::Connect);
                }
                w.publish();
                w.run(rx);
            })
            .expect("spawning worker thread");
        tx
    }

    fn run(&mut self, rx: Receiver<Command>) {
        let mut next_poll = Instant::now() + POLL_PERIOD;
        loop {
            match rx.recv_timeout(next_poll.saturating_duration_since(Instant::now())) {
                Ok(first) => {
                    let mut batch = vec![first];
                    batch.extend(rx.try_iter());
                    // Drop commands superseded by a later one with the same key.
                    let keep: Vec<bool> = (0..batch.len())
                        .map(|i| match batch[i].coalesce_key() {
                            Some(k) => !batch[i + 1..].iter().any(|c| c.coalesce_key() == Some(k)),
                            None => true,
                        })
                        .collect();
                    for (cmd, keep) in batch.into_iter().zip(keep) {
                        if !keep {
                            continue;
                        }
                        if matches!(cmd, Command::Shutdown) {
                            self.disconnect();
                            return;
                        }
                        self.handle(cmd);
                    }
                    self.publish();
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    self.disconnect();
                    return;
                }
            }
            if Instant::now() >= next_poll {
                next_poll = Instant::now() + POLL_PERIOD;
                if self.conn.is_some() {
                    self.poll();
                    self.publish();
                }
            }
        }
    }

    fn message(&mut self, text: impl Into<String>, error: bool) {
        let text = text.into();
        if error {
            log::error!("{text}");
        } else {
            log::info!("{text}");
        }
        (self.emit)(Event::Message { text, error });
    }

    fn publish(&mut self) {
        let state = json!({
            "connected": self.conn.is_some(),
            "simulated": self.simulate,
            "port": self.port.as_ref().map(|p| p.path.clone()).unwrap_or_default(),
            "serial": self.port.as_ref().map(|p| p.serial.clone()).unwrap_or_default(),
            "flashing": self.flash.is_some(),
            "hubs": self.hubs.iter().map(HubState::to_json).collect::<Vec<_>>(),
        });
        (self.emit)(Event::State(state));
    }

    fn handle(&mut self, cmd: Command) {
        let result = match cmd {
            Command::Connect => self.connect(),
            Command::Disconnect => {
                self.disconnect();
                Ok(())
            }
            Command::EmergencyStop => {
                self.stop_all();
                self.message("Emergency stop: all motors and servos disabled", false);
                Ok(())
            }
            Command::Flash { path } => self.flash(path),
            cmd => self.hub_command(cmd),
        };
        if let Err(e) = result {
            self.message(format!("{e:#}"), true);
        }
    }

    fn connect(&mut self) -> anyhow::Result<()> {
        if self.conn.is_some() {
            return Ok(());
        }
        let mut conn = if self.simulate {
            self.port = Some(hub::PortInfo { path: "simulator".into(), serial: "DQSIM001".into() });
            Connection::new(Box::new(SimTransport::new()))
        } else {
            let port = hub::list_hub_ports().into_iter().next().ok_or_else(|| {
                anyhow::anyhow!("No REV Expansion Hub found. Check the USB cable, and that the hub is powered.")
            })?;
            let transport = SerialTransport::open(&port.path).map_err(|e| match e {
                HubError::Serial(ref se) if matches!(se.kind, serialport::ErrorKind::Io(std::io::ErrorKind::PermissionDenied)) => {
                    anyhow::anyhow!(
                        "Permission denied opening {}. Install packaging/70-rev-hub.rules (see README) and replug the hub.",
                        port.path
                    )
                }
                e => anyhow::anyhow!("Opening {}: {e}", port.path),
            })?;
            self.port = Some(port);
            Connection::new(Box::new(transport))
        };

        let found = conn.discover()?;
        if found.is_empty() {
            self.port = None;
            anyhow::bail!("The serial port opened but no hub answered discovery. Is the hub powered by a battery?");
        }
        let mut hubs = Vec::new();
        for d in found {
            let mut h = HubState::new(d.address, d.is_parent);
            let a = d.address;
            h.firmware = conn.version_string(a).unwrap_or_else(|e| format!("unknown ({e})"));
            for m in 0..4 {
                conn.set_motor_mode(a, m, 0, true)?;
                conn.set_motor_power(a, m, 0)?;
            }
            for s in 0..6 {
                conn.set_servo_period(a, s, SERVO_PERIOD_US)?;
            }
            hubs.push(h);
        }
        let n = hubs.len();
        self.hubs = hubs;
        self.conn = Some(conn);
        self.failed_polls = 0;
        self.tick = 0;
        self.poll();
        self.message(format!("Connected to {n} hub{}", if n == 1 { "" } else { "s" }), false);
        Ok(())
    }

    fn stop_all(&mut self) {
        let Some(conn) = self.conn.as_mut() else { return };
        for h in &mut self.hubs {
            let a = h.address;
            for (m, motor) in h.motors.iter_mut().enumerate() {
                let _ = conn.set_motor_power(a, m as u8, 0);
                let _ = conn.set_motor_enable(a, m as u8, false);
                motor.power = 0;
                motor.enabled = false;
            }
            for (s, servo) in h.servos.iter_mut().enumerate() {
                let _ = conn.set_servo_enable(a, s as u8, false);
                servo.enabled = false;
            }
            for slot in h.i2c.iter_mut().chain(std::iter::once(&mut h.imu)) {
                slot.polling = false;
            }
            let _ = conn.fail_safe(a);
        }
    }

    fn disconnect(&mut self) {
        if self.conn.is_none() {
            return;
        }
        self.stop_all();
        self.conn = None;
        self.hubs.clear();
        self.message("Disconnected", false);
    }

    fn hub_command(&mut self, cmd: Command) -> anyhow::Result<()> {
        let (Some(conn), hub_idx) = (self.conn.as_mut(), hub_index(&cmd)) else {
            anyhow::bail!("Not connected");
        };
        let h = self.hubs.get_mut(hub_idx).ok_or_else(|| anyhow::anyhow!("No hub #{hub_idx}"))?;
        let a = h.address;
        match cmd {
            Command::SetMotorPower { motor, power, .. } => {
                let m = &mut h.motors[motor as usize];
                conn.set_motor_power(a, motor, power)?;
                if !m.enabled && power != 0 {
                    conn.set_motor_enable(a, motor, true)?;
                    m.enabled = true;
                }
                m.power = power;
            }
            Command::SetMotorFloat { motor, float, .. } => {
                conn.set_motor_mode(a, motor, 0, float)?;
                h.motors[motor as usize].float = float;
            }
            Command::ResetEncoder { motor, .. } => conn.reset_encoder(a, motor)?,
            Command::SetServo { servo, pulse_us, enabled, .. } => {
                let s = &mut h.servos[servo as usize];
                conn.set_servo_pulse(a, servo, pulse_us)?;
                if s.enabled != enabled {
                    conn.set_servo_enable(a, servo, enabled)?;
                }
                *s = Servo { pulse_us, enabled };
            }
            Command::SetDioDirection { pin, output, .. } => {
                conn.set_dio_direction(a, pin, output)?;
                h.dio[pin as usize].output = output;
            }
            Command::SetDioOutput { pin, value, .. } => {
                conn.set_dio_output(a, pin, value)?;
                h.dio[pin as usize].value = value;
            }
            Command::I2cDetect { bus, .. } => {
                let slot = &mut h.i2c[bus as usize];
                *slot = I2cSlot::default();
                match sensors::detect_and_init(conn, a, bus)? {
                    Some(kind) => {
                        slot.sensor = Some(kind);
                        slot.message = format!("{} found", kind.label());
                    }
                    None => slot.message = "No supported sensor found".into(),
                }
            }
            Command::I2cPoll { bus, enabled, .. } => h.i2c[bus as usize].polling = enabled && h.i2c[bus as usize].sensor.is_some(),
            Command::ImuInit { .. } => {
                let mut dev = sensors::Device::new(conn, a, 0, sensors::bno055::ADDRESS);
                h.imu = I2cSlot::default();
                if !sensors::bno055::detect(&mut dev) {
                    h.imu.message = "No BNO055 found on I2C bus 0 (hubs made since 2022 use a BHI260AP IMU, which is not supported)".into();
                } else if sensors::bno055::init(&mut dev)? {
                    h.imu.sensor = Some(SensorKind::Imu);
                    h.imu.polling = true;
                    h.imu.message = "IMU fusion running".into();
                } else {
                    h.imu.sensor = Some(SensorKind::Imu);
                    h.imu.message = "IMU found, but sensor fusion did not start".into();
                }
            }
            Command::ImuPoll { enabled, .. } => h.imu.polling = enabled && h.imu.sensor.is_some(),
            Command::SetAddress { address, .. } => {
                anyhow::ensure!((1..=254).contains(&address), "Hub addresses must be between 1 and 254");
                anyhow::ensure!(!self.hubs.iter().any(|o| o.address == address), "Another hub already uses address {address}");
                conn.set_module_address(a, address)?;
                self.hubs[hub_idx].address = address;
                self.message(format!("Hub address changed from {a} to {address}"), false);
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    fn poll(&mut self) {
        let Some(conn) = self.conn.as_mut() else { return };
        self.tick = self.tick.wrapping_add(1);
        let slow = self.tick % SLOW_POLL_EVERY == 1;
        let mut ok = true;
        for h in &mut self.hubs {
            let a = h.address;
            let r: hub::Result<()> = (|| {
                let bulk = conn.bulk_input(a)?;
                for (i, m) in h.motors.iter_mut().enumerate() {
                    m.position = bulk.encoders[i];
                    m.velocity = bulk.velocities[i];
                    m.current_ma = conn.motor_current(a, i as u8)?;
                }
                for (i, d) in h.dio.iter_mut().enumerate() {
                    if !d.output {
                        d.value = bulk.digital_inputs & (1 << i) != 0;
                    }
                }
                for (i, mv) in h.analog_mv.iter_mut().enumerate() {
                    *mv = conn.adc(a, adc::ANALOG_0 + i as u8)?;
                }
                if slow {
                    conn.keep_alive(a)?;
                    h.battery_mv = conn.adc(a, adc::BATTERY_VOLTAGE)?;
                    h.monitor_5v_mv = conn.adc(a, adc::MONITOR_5V)?;
                    h.temperature_dc = conn.adc(a, adc::CONTROLLER_TEMPERATURE)?;
                    (h.status, h.motor_alerts) = conn.module_status(a)?;
                }
                Ok(())
            })();
            if let Err(e) = r {
                log::warn!("polling hub {a}: {e}");
                ok = false;
            }
            for (bus, slot) in h.i2c.iter_mut().enumerate().map(|(b, s)| (b as u8, s)).chain(std::iter::once((0, &mut h.imu))) {
                if let (true, Some(kind)) = (slot.polling, slot.sensor) {
                    match sensors::read(conn, a, bus, kind) {
                        Ok(v) => slot.reading = Some(v),
                        Err(e) => {
                            slot.message = format!("Read failed: {e}");
                            slot.polling = false;
                        }
                    }
                }
            }
        }
        self.failed_polls = if ok { 0 } else { self.failed_polls + 1 };
        if self.failed_polls >= MAX_FAILED_POLLS {
            self.conn = None;
            self.hubs.clear();
            self.message("Lost connection to the hub", true);
        }
    }

    fn flash(&mut self, path: PathBuf) -> anyhow::Result<()> {
        anyhow::ensure!(!self.simulate, "Firmware update is not available in simulator mode");
        let image = firmware::validate_image(&path)?;
        let port = match &self.port {
            Some(p) if self.conn.is_some() => p.clone(),
            _ => hub::list_hub_ports().into_iter().next().ok_or_else(|| anyhow::anyhow!("No hub serial port found"))?,
        };
        self.disconnect();
        self.message(format!("Programming {} on {} — do not remove power", path.display(), port.path), false);
        self.flash = Some((0.0, String::new()));
        self.publish();
        let emit = &mut self.emit;
        let result = firmware::flash(&port.path, &port.serial, &image, &mut |fraction, message| {
            emit(Event::FlashProgress { fraction, message })
        });
        self.flash = None;
        result
    }
}

fn hub_index(cmd: &Command) -> usize {
    match *cmd {
        Command::SetMotorPower { hub, .. }
        | Command::SetMotorFloat { hub, .. }
        | Command::ResetEncoder { hub, .. }
        | Command::SetServo { hub, .. }
        | Command::SetDioDirection { hub, .. }
        | Command::SetDioOutput { hub, .. }
        | Command::I2cDetect { hub, .. }
        | Command::I2cPoll { hub, .. }
        | Command::ImuInit { hub }
        | Command::ImuPoll { hub, .. }
        | Command::SetAddress { hub, .. } => hub,
        _ => usize::MAX,
    }
}
