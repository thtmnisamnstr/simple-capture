#!/usr/bin/env bash
# Makes the apt/dnf repository signing key (Ed25519, no passphrase, signing only, 5 years) in a
# throwaway keyring and stores it as the LINUX_REPO_GPG_KEY secret with gh. The private key exists
# only in that secret afterwards; keep the printed revocation certificate somewhere safe.
#   scripts/linux-repos/new-key.sh
set -euo pipefail
command -v gh >/dev/null || { echo "new-key.sh: needs gh" >&2; exit 1; }
export GNUPGHOME
GNUPGHOME=$(mktemp -d)
trap 'rm -rf "$GNUPGHOME"' EXIT
gpg --batch --passphrase '' --quick-gen-key "simple-capture packages <packages@simplecapture.invalid>" ed25519 sign 5y
fpr=$(gpg --list-secret-keys --with-colons | awk -F: '/^fpr/ {print $10; exit}')
gpg --batch --armor --export-secret-keys "$fpr" | gh secret set LINUX_REPO_GPG_KEY
echo "new-key.sh: LINUX_REPO_GPG_KEY set; fingerprint $fpr"
echo "Revocation certificate (store it offline):"
cat "$GNUPGHOME/openpgp-revocs.d/$fpr.rev"
