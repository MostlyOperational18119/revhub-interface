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

`revhub-interface --list-ports` prints the detected hubs and whether serial and
USB access are granted, without opening the UI.

## Flatpak (Fedora Kinoite / Silverblue)

Build and install for the current user (needs `org.flatpak.Builder` from Flathub;
the KDE 6.10 SDK and Rust extension are installed automatically):

```sh
cd packaging/flatpak
flatpak run org.flatpak.Builder --user --install --force-clean --install-deps-from=flathub \
    build-dir io.github.ishaderdevicemgr.RevHubInterface.yml
```

To produce a single-file bundle for other laptops:

```sh
flatpak run org.flatpak.Builder --repo=repo --force-clean build-dir io.github.ishaderdevicemgr.RevHubInterface.yml
flatpak build-bundle repo revhub-interface.flatpak io.github.ishaderdevicemgr.RevHubInterface \
    --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo
# on each laptop:
flatpak install --user revhub-interface.flatpak
```

The sandbox gets `--device=all` (Flatpak has no narrower permission for serial
ports), but host device permissions still apply, so each laptop also needs the
udev rule, installed once by an admin. This works on immutable systems like
Kinoite: only `/usr` is read-only, while `/etc` is writable and preserved across
updates and rollbacks, so no `rpm-ostree` layering or reboot is needed. The
Flatpak bundles the rule, so the repo isn't needed on each laptop:

```sh
flatpak run --no-documents-portal --command=cat io.github.ishaderdevicemgr.RevHubInterface /app/share/revhub-interface/70-rev-hub.rules \
  | sudo tee /etc/udev/rules.d/70-rev-hub.rules
sudo udevadm control --reload && sudo udevadm trigger
```

Then replug the hub and check with
`flatpak run io.github.ishaderdevicemgr.RevHubInterface --list-ports`.

Don't use the `dialout` group on Kinoite: system groups live in
`/usr/lib/group`, so `usermod -aG dialout` fails, and the group doesn't cover
the USB access firmware updates need anyway.

After changing dependencies, regenerate the offline crate list:

```sh
python3 packaging/flatpak/generate-cargo-sources.py Cargo.lock > packaging/flatpak/cargo-sources.json
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
