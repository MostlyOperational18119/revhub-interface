#!/usr/bin/env bash
# One-time setup: create the GPG key that signs the Flatpak repository.
#
# The private key goes into the FLATPAK_GPG_PRIVATE_KEY repository secret (via
# the gh CLI when it is logged in) and a backup file in your home directory.
# Keep the backup somewhere safe: if the key is lost, every laptop has to
# remove and re-add the remote.
set -euo pipefail

backup="$HOME/revhub-flatpak-signing-key.asc"
[[ -e $backup ]] && { echo "$backup already exists; refusing to overwrite it" >&2; exit 1; }

GNUPGHOME=$(mktemp -d)
export GNUPGHOME
trap 'rm -rf "$GNUPGHOME"' EXIT

gpg --batch --passphrase '' --quick-gen-key \
    "REV Hub Interface Flatpak repo <revhub-interface@users.noreply.github.com>" rsa4096 sign never
(umask 077 && gpg --armor --export-secret-keys > "$backup")
echo "Private key backed up to $backup"

if gh auth status >/dev/null 2>&1; then
    gh secret set FLATPAK_GPG_PRIVATE_KEY --repo MostlyOperational18119/revhub-interface < "$backup"
    echo "Stored as the FLATPAK_GPG_PRIVATE_KEY repository secret."
else
    echo "gh is not logged in. Add the key as a repository secret named FLATPAK_GPG_PRIVATE_KEY:"
    echo "  gh auth login && gh secret set FLATPAK_GPG_PRIVATE_KEY --repo MostlyOperational18119/revhub-interface < $backup"
    echo "or paste the file's contents under Settings > Secrets and variables > Actions."
fi
