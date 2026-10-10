# REV Hub Interface for Linux

A native Rust + Qt 6 / Kirigami port of REV Robotics' Windows-only **REV Hub Interface 1.2.0**
(BSD-3-Clause), built with [Qt Bridge for Rust](https://github.com/qt/qtbridge-rust).

Drive DC motors and servos, read I2C sensors, digital and analog IO, set hub
addresses, and update Expansion Hub firmware directly from a Linux PC over USB.

## Requirements

- Rust ≥ 1.88, a C++ toolchain, Qt ≥ 6.10 with private headers, Kirigami (KF6), libusb, libudev.
  Fedora: `sudo dnf install qt6-qtbase-devel qt6-qtbase-private-devel qt6-qtdeclarative-devel kf6-kirigami libusb1-devel systemd-devel`

## Hub access (one-time)

```sh
sudo cp packaging/70-rev-hub.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger
```

Then unplug and replug the hub. This grants the logged-in user access to the
hub's serial port and to the raw USB device used to enter the bootloader.

## Running

```sh
cargo run --release              # real hardware
cargo run --release -- --simulate  # two simulated hubs with sensors, no hardware needed
```

Press **Esc** (or E-Stop) at any time to stop all motors and servos.
Motors and servos need 12 V battery power; USB alone only powers the logic.

## Features

| Area | Notes |
| --- | --- |
| DC motors | Open-loop power (-1…1), brake/coast at zero, encoder position, velocity, current |
| Servos | 500–2500 µs pulses, FTC position (0…1), enable/disable |
| I2C | Auto-detect REV 2m Distance Sensor (VL53L0X), Color Sensor V3 (APDS-9151), Color Sensor V2 |
| IMU | Built-in BNO055 (heading/roll/pitch, gravity). BHI260AP hubs (2022+) are not supported |
| IO | 4 analog inputs, 8 digital pins as inputs or outputs |
| Hub | Discovery of RS485 daisy-chained hubs, address changes, battery/5 V/temperature, alerts |
| Firmware | Native TI serial-bootloader flasher; replaces the Windows `sflash.exe` |

## Layout

- `src/protocol.rs` – packet framing for the REV hub ("Lynx") serial protocol
- `src/hub.rs` – serial transport and typed hub commands
- `src/sensors.rs` – I2C sensor drivers
- `src/firmware.rs` – FT230X CBUS bootloader entry and the TI bootloader protocol
- `src/worker.rs` – background thread: command queue, 10 Hz polling, keep-alive
- `src/sim.rs` – simulated hubs for `--simulate` and the end-to-end test
- `src/main.rs` – QML backend singleton
- `src/qml/` – Kirigami UI

The decompiled original (Python 2.7, recovered with pyinstxtractor + pycdc) is in `../decompiled/` for reference.
