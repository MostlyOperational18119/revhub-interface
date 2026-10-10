//! REV Hub Interface: a Linux-native Qt Quick / Kirigami tool for driving REV Expansion Hubs.

mod firmware;
mod hub;
mod protocol;
mod sensors;
mod sim;
mod worker;

use crossbeam_channel::Sender;
use qtbridge::{QApp, QmlObject, include_bytes_qml, invoke_method, qobject};
use serde_json::{Value, json};
use worker::{Command, Event, Worker};

pub struct Backend {
    state: Value,
    flash_progress: f64,
    flash_message: String,
    worker: Option<Sender<Command>>,
}

impl Default for Backend {
    fn default() -> Self {
        Self {
            state: json!({ "connected": false, "hubs": [] }),
            flash_progress: 0.0,
            flash_message: String::new(),
            worker: None,
        }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.send(Command::Shutdown);
    }
}

impl Backend {
    fn send(&self, cmd: Command) {
        if let Some(tx) = &self.worker {
            let _ = tx.send(cmd);
        }
    }
}

fn clamp_u8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

fn hub(v: i32) -> usize {
    v.max(0) as usize
}

#[qobject(Singleton)]
impl Backend {
    qproperty!("state", Member = state, Notify = state_changed);
    qproperty!("flashProgress", Member = flash_progress, Notify = flash_changed);
    qproperty!("flashMessage", Member = flash_message, Notify = flash_changed);

    #[qsignal]
    fn state_changed(&mut self);

    #[qsignal]
    fn flash_changed(&mut self);

    /// Passive notification for the UI (status messages and errors).
    #[qsignal(qml_name = "notify")]
    fn notify(&mut self, text: &String, error: bool);

    /// Starts the hub worker thread. Called once by QML when the window is ready.
    #[qslot]
    fn start(&mut self) {
        if self.worker.is_some() {
            return;
        }
        let simulate = std::env::args().any(|a| a == "--simulate");
        let invoker = self.get_qml_method_invoker();
        self.worker = Some(Worker::spawn(
            simulate,
            Box::new(move |event| match event {
                Event::State(v) => {
                    invoke_method!(invoker, "apply_state", v.to_string());
                }
                Event::Message { text, error } => {
                    invoke_method!(invoker, "apply_message", text, error);
                }
                Event::FlashProgress { fraction, message } => {
                    invoke_method!(invoker, "apply_flash", fraction, message);
                }
            }),
        ));
    }

    #[qslot]
    fn apply_state(&mut self, json: &String) {
        if let Ok(v) = serde_json::from_str(json) {
            self.state = v;
            self.state_changed();
        }
    }

    #[qslot]
    fn apply_message(&mut self, text: &String, error: bool) {
        self.notify(text, error);
    }

    #[qslot]
    fn apply_flash(&mut self, fraction: f64, message: &String) {
        self.flash_progress = fraction;
        self.flash_message = message.clone();
        self.flash_changed();
    }

    #[qslot(qml_name = "connectHubs")]
    fn connect_hubs(&self) {
        self.send(Command::Connect);
    }

    #[qslot(qml_name = "disconnectHubs")]
    fn disconnect_hubs(&self) {
        self.send(Command::Disconnect);
    }

    #[qslot(qml_name = "emergencyStop")]
    fn emergency_stop(&self) {
        self.send(Command::EmergencyStop);
    }

    /// `power` is the FTC SDK style value in -1.0..=1.0.
    #[qslot(qml_name = "setMotorPower")]
    fn set_motor_power(&self, hub_index: i32, motor: i32, power: f64) {
        let power = (power.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        self.send(Command::SetMotorPower { hub: hub(hub_index), motor: clamp_u8(motor), power });
    }

    #[qslot(qml_name = "setMotorFloat")]
    fn set_motor_float(&self, hub_index: i32, motor: i32, float: bool) {
        self.send(Command::SetMotorFloat { hub: hub(hub_index), motor: clamp_u8(motor), float });
    }

    #[qslot(qml_name = "resetEncoder")]
    fn reset_encoder(&self, hub_index: i32, motor: i32) {
        self.send(Command::ResetEncoder { hub: hub(hub_index), motor: clamp_u8(motor) });
    }

    #[qslot(qml_name = "setServo")]
    fn set_servo(&self, hub_index: i32, servo: i32, pulse_us: i32, enabled: bool) {
        let pulse_us = pulse_us.clamp(500, 2500) as u16;
        self.send(Command::SetServo { hub: hub(hub_index), servo: clamp_u8(servo), pulse_us, enabled });
    }

    #[qslot(qml_name = "setDioDirection")]
    fn set_dio_direction(&self, hub_index: i32, pin: i32, output: bool) {
        self.send(Command::SetDioDirection { hub: hub(hub_index), pin: clamp_u8(pin), output });
    }

    #[qslot(qml_name = "setDioOutput")]
    fn set_dio_output(&self, hub_index: i32, pin: i32, value: bool) {
        self.send(Command::SetDioOutput { hub: hub(hub_index), pin: clamp_u8(pin), value });
    }

    #[qslot(qml_name = "i2cDetect")]
    fn i2c_detect(&self, hub_index: i32, bus: i32) {
        self.send(Command::I2cDetect { hub: hub(hub_index), bus: clamp_u8(bus) });
    }

    #[qslot(qml_name = "i2cPoll")]
    fn i2c_poll(&self, hub_index: i32, bus: i32, enabled: bool) {
        self.send(Command::I2cPoll { hub: hub(hub_index), bus: clamp_u8(bus), enabled });
    }

    #[qslot(qml_name = "imuInit")]
    fn imu_init(&self, hub_index: i32) {
        self.send(Command::ImuInit { hub: hub(hub_index) });
    }

    #[qslot(qml_name = "imuPoll")]
    fn imu_poll(&self, hub_index: i32, enabled: bool) {
        self.send(Command::ImuPoll { hub: hub(hub_index), enabled });
    }

    #[qslot(qml_name = "setAddress")]
    fn set_address(&self, hub_index: i32, address: i32) {
        self.send(Command::SetAddress { hub: hub(hub_index), address: clamp_u8(address) });
    }

    /// Accepts a local path or a `file://` URL from a QML FileDialog.
    #[qslot(qml_name = "flashFirmware")]
    fn flash_firmware(&self, url: &String) {
        let path = url.strip_prefix("file://").unwrap_or(url);
        self.send(Command::Flash { path: path.into() });
    }


    #[qslot(qml_name = "appVersion")]
    fn app_version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if std::env::var_os("QT_QUICK_CONTROLS_STYLE").is_none() {
        // org.kde.desktop needs a QApplication; QtBridge creates a QGuiApplication.
        // SAFETY: called before any other thread is started.
        unsafe { std::env::set_var("QT_QUICK_CONTROLS_STYLE", "org.kde.breeze") };
    }
    include_bytes_qml!("qml");
    QApp::new()
        .application_name("REV Hub Interface")
        .register::<Backend>()
        .load_qml_from_file("qrc:/qml/Main.qml")
        .run();
}
