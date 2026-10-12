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
Press **Esc** (or E-Stop) at any time to stop all motors and servos.
Motors and servos need 12 V battery power; USB alone only powers the logic.

## Flatpak (Fedora Kinoite / Silverblue)

### Installing on a team laptop

The app is published as a signed Flatpak repository on GitHub Pages
(<https://mostlyoperational18119.github.io/revhub-interface/>). Install it once; this adds the `revhub` remote, and
Discover (or `flatpak update`) then picks up every new release:

```sh
flatpak install https://mostlyoperational18119.github.io/revhub-interface/io.github.mostlyoperational18119.RevHubInterface.flatpakref
```

The KDE runtime comes from Flathub and is added automatically if needed.

Each laptop also needs the hub udev rule, installed once by an admin. The
sandbox has `--device=all` (Flatpak has no narrower permission for serial
ports), but host device permissions still apply. This works on immutable
systems like Kinoite: only `/usr` is read-only, while `/etc` is writable and
preserved across updates and rollbacks, so no `rpm-ostree` layering or reboot is
needed. The Flatpak bundles the rule:

```sh
flatpak run --no-documents-portal --command=cat io.github.mostlyoperational18119.RevHubInterface /app/share/revhub-interface/70-rev-hub.rules \
  | sudo tee /etc/udev/rules.d/70-rev-hub.rules
sudo udevadm control --reload && sudo udevadm trigger
```

Then replug the hub and check with `flatpak run io.github.mostlyoperational18119.RevHubInterface --list-ports`.

Don't use the `dialout` group on Kinoite: system groups live in
`/usr/lib/group`, so `usermod -aG dialout` fails, and the group doesn't cover
the USB access firmware updates need anyway.

### Publishing a release

`.github/workflows/flatpak-repo.yml` builds x86_64 and aarch64, signs the
repository with the team's GPG key and deploys it to GitHub Pages. It runs
when a `v*` tag is pushed, or manually from the Actions tab.

One-time setup:

1. Create the signing key: `packaging/flatpak/make-signing-key.sh`. It stores the
   private key as the `FLATPAK_GPG_PRIVATE_KEY` repository secret (if `gh` is logged
   in) and writes a backup to `~/revhub-flatpak-signing-key.asc`. Keep that backup
   safe: if the key is lost, every laptop has to remove and re-add the remote.
2. Under Settings → Pages, set Source to **GitHub Actions**.

For each release:

1. Bump `version` in `Cargo.toml` and add a `<release version="X.Y.Z" date="..."/>`
   entry at the top of `<releases>` in
   `packaging/flatpak/io.github.mostlyoperational18119.RevHubInterface.metainfo.xml` (CI checks both match the tag).
2. Commit, then tag and push: `git tag vX.Y.Z && git push origin main vX.Y.Z`.

### Building locally

Build and install for the current user (needs `org.flatpak.Builder` from Flathub;
the KDE 6.10 SDK and Rust extension are installed automatically):

```sh
cd packaging/flatpak
flatpak run org.flatpak.Builder --user --install --force-clean --install-deps-from=flathub \
    build-dir io.github.mostlyoperational18119.RevHubInterface.yml
```

After changing dependencies, regenerate the offline crate list:

```sh
python3 packaging/flatpak/generate-cargo-sources.py Cargo.lock > packaging/flatpak/cargo-sources.json
```

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
- `packaging/flatpak/` – Flatpak manifest, metadata, crate list generator, repo signing/publishing scripts
- `.github/workflows/flatpak-repo.yml` – builds and publishes the Flatpak repository to GitHub Pages

The decompiled original (Python 2.7, recovered with pyinstxtractor + pycdc) is in `../decompiled/` for reference.
