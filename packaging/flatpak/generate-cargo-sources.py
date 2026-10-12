#!/usr/bin/env python3
"""Generate flatpak-builder sources that vendor every crate in Cargo.lock.

Flatpak builds run without network access, so each crates.io dependency is
downloaded by flatpak-builder (checksum-verified) into cargo/vendor/, and a
cargo config redirects crates.io to that directory.

Usage: generate-cargo-sources.py ../../Cargo.lock > cargo-sources.json
"""

import json
import sys
import tomllib

CRATES_IO = "registry+https://github.com/rust-lang/crates.io-index"
VENDOR_DIR = "cargo/vendor"


def main() -> None:
    with open(sys.argv[1] if len(sys.argv) > 1 else "Cargo.lock", "rb") as f:
        lock = tomllib.load(f)

    sources = []
    for pkg in lock["package"]:
        source = pkg.get("source")
        if source is None:
            continue  # the workspace's own crates
        if source != CRATES_IO:
            sys.exit(f"unsupported source for {pkg['name']}: {source}")
        name, version, checksum = pkg["name"], pkg["version"], pkg["checksum"]
        dest = f"{VENDOR_DIR}/{name}-{version}"
        sources.append({
            "type": "archive",
            "archive-type": "tar-gzip",
            "url": f"https://static.crates.io/crates/{name}/{name}-{version}.crate",
            "sha256": checksum,
            "dest": dest,
        })
        sources.append({
            "type": "inline",
            "contents": json.dumps({"package": checksum, "files": {}}),
            "dest": dest,
            "dest-filename": ".cargo-checksum.json",
        })

    sources.append({
        "type": "inline",
        "contents": (
            '[source.vendored-sources]\n'
            f'directory = "{VENDOR_DIR}"\n\n'
            '[source.crates-io]\n'
            'replace-with = "vendored-sources"\n'
        ),
        "dest": "cargo",
        "dest-filename": "config.toml",
    })
    json.dump(sources, sys.stdout, indent=2)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
