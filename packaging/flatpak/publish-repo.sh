#!/usr/bin/env bash
# Merge per-architecture flatpak-builder repos into one GPG-signed OSTree repo,
# plus the .flatpakrepo/.flatpakref files clients use, ready to serve as a
# static site (GitHub Pages).
#
# Usage: publish-repo.sh OUT_DIR BASE_URL GPG_KEY_ID SRC_REPO...
#
# GNUPGHOME must contain the private signing key. Needs flatpak, ostree and gpg
# (CI installs them from Ubuntu). Without an ostree CLI, run it inside
# org.flatpak.Builder, which bundles all three, and stop the sandboxed
# gpg-agent afterwards or `flatpak run` will not exit:
#   flatpak run --command=bash org.flatpak.Builder -c \
#     'trap "gpgconf --kill gpg-agent" EXIT; packaging/flatpak/publish-repo.sh ...'
set -euo pipefail

out=$1
base_url=${2%/}
key=$3
shift 3

app_id=io.github.mostlyoperational18119.RevHubInterface
branch=stable
here=$(dirname "$(realpath "$0")")
gpg_args=(--gpg-sign="$key")
[[ -n ${GNUPGHOME:-} ]] && gpg_args+=(--gpg-homedir="$GNUPGHOME")

rm -rf "$out"
mkdir -p "$out"
ostree init --mode=archive-z2 --repo="$out/repo"

# Only the app itself; the .Debug extensions would double the download size.
found=0
for src in "$@"; do
    for ref in $(ostree refs --repo="$src" | grep -E "^app/$app_id/[^/]+/$branch\$"); do
        echo "Publishing $ref from $src"
        flatpak build-commit-from --src-repo="$src" --no-update-summary "${gpg_args[@]}" "$out/repo" "$ref"
        found=$((found + 1))
    done
done
((found > 0)) || { echo "no app/$app_id/*/$branch refs found in: $*" >&2; exit 1; }

flatpak build-update-repo "${gpg_args[@]}" --generate-static-deltas --prune \
    --title="REV Hub Interface" --default-branch="$branch" "$out/repo"

gpg_key=$(gpg --export "$key" | base64 -w0)
[[ -n $gpg_key ]] || { echo "could not export public key $key" >&2; exit 1; }

cat > "$out/revhub.flatpakrepo" <<EOF
[Flatpak Repo]
Title=REV Hub Interface
Url=$base_url/repo/
Homepage=https://github.com/MostlyOperational18119/revhub-interface
Comment=REV Expansion Hub tools for FTC teams
Icon=$base_url/$app_id.svg
DefaultBranch=$branch
GPGKey=$gpg_key
EOF

# One-step install: adds the remote, installs the app and pulls the KDE runtime from Flathub.
cat > "$out/$app_id.flatpakref" <<EOF
[Flatpak Ref]
Name=$app_id
Branch=$branch
Title=REV Hub Interface
Url=$base_url/repo/
SuggestRemoteName=revhub
RuntimeRepo=https://dl.flathub.org/repo/flathub.flatpakrepo
IsRuntime=false
GPGKey=$gpg_key
EOF

cp "$here/$app_id.svg" "$out/"
cat > "$out/index.html" <<EOF
<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>REV Hub Interface Flatpak repository</title></head>
<body style="font-family: sans-serif; max-width: 48rem; margin: 2rem auto; padding: 0 1rem">
<h1>REV Hub Interface</h1>
<p>Flatpak repository for <a href="https://github.com/MostlyOperational18119/revhub-interface">revhub-interface</a>.</p>
<h2>Install</h2>
<pre>flatpak install $base_url/$app_id.flatpakref</pre>
<p>Updates then arrive through Discover or <code>flatpak update</code>.</p>
<p>Each computer also needs the hub udev rule, once; see the
<a href="https://github.com/MostlyOperational18119/revhub-interface#installing-on-a-team-laptop">README</a>.</p>
</body>
</html>
EOF

echo "Repository written to $out ($found ref(s))"
