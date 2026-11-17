#!/usr/bin/env bash

set -euo pipefail

readonly REPOSITORY="inttegro/simile"
readonly INSTALLER_ASSET="simile-cli-installer.sh"

fail() {
  printf 'simile installer: %s\n' "$1" >&2
  exit 1
}

version="${SIMILE_VERSION:-latest}"
if [[ "$version" == "latest" ]]; then
  release_path="latest/download"
elif [[ "$version" =~ ^v?[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$ ]]; then
  # Release tags use a v prefix. Normalizing here lets SIMILE_VERSION accept either 1.2.3 or
  # v1.2.3 without allowing path traversal or arbitrary release URLs.
  release_path="download/v${version#v}"
else
  fail "SIMILE_VERSION must be 'latest' or a semantic version such as 0.1.1"
fi

command -v curl >/dev/null 2>&1 || fail "curl is required"

temporary_directory="$(mktemp -d 2>/dev/null || mktemp -d -t simile-installer)"
readonly temporary_directory
readonly installer_path="$temporary_directory/$INSTALLER_ASSET"

cleanup() {
  # The downloaded program is executable code, so do not leave it behind after success or failure.
  rm -f "$installer_path"
  rmdir "$temporary_directory" 2>/dev/null || true
}
trap cleanup EXIT

readonly installer_url="https://github.com/$REPOSITORY/releases/$release_path/$INSTALLER_ASSET"

# TLS 1.2+ and HTTPS-only redirects prevent accidental downgrade. Downloading before execution
# also makes a failed or partial transfer fatal instead of feeding partial input to a shell.
curl \
  --proto '=https' \
  --proto-redir '=https' \
  --tlsv1.2 \
  --fail \
  --location \
  --silent \
  --show-error \
  --output "$installer_path" \
  "$installer_url"

# cargo-dist's versioned installer selects the platform archive and verifies its release checksum.
/bin/sh "$installer_path"
