#!/usr/bin/env sh
set -eu

REPO="${BEE_REPO:-hembrow-innovations/bee}"
INSTALL_DIR="${BEE_INSTALL_DIR:-${HOME}/.local/bin}"
GITHUB_API="${GITHUB_API:-https://api.github.com}"
GITHUB_URL="${GITHUB_URL:-https://github.com}"

err() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || err "required command not found: $1"
}

detect_triple() {
  os="$(uname -s 2>/dev/null || true)"
  arch="$(uname -m 2>/dev/null || true)"

  case "${OS:-}" in
    Windows_NT) err "Windows is not supported; install on macOS" ;;
  esac

  case "$os" in
    MINGW*|MSYS*|CYGWIN*|Windows_NT)
      err "Windows is not supported; install on macOS"
      ;;
    Darwin)
      case "$arch" in
        arm64|aarch64) printf '%s\n' "aarch64-apple-darwin" ;;
        x86_64) printf '%s\n' "x86_64-apple-darwin" ;;
        *) err "unsupported macOS architecture: ${arch}" ;;
      esac
      ;;
    *)
      err "unsupported OS: ${os:-unknown} (supported: macOS)"
      ;;
  esac
}

normalize_version() {
  v="$1"
  case "$v" in
    v*|V*) printf '%s\n' "${v#?}" ;;
    *) printf '%s\n' "$v" ;;
  esac
}

latest_version() {
  need_cmd curl
  json="$(curl -fsSL "${GITHUB_API}/repos/${REPO}/releases/latest")" || \
    err "failed to fetch latest release from ${GITHUB_API}/repos/${REPO}/releases/latest"
  tag="$(printf '%s\n' "$json" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -1)"
  [ -n "$tag" ] || err "could not parse tag_name from latest release JSON"
  normalize_version "$tag"
}

download() {
  url="$1"
  dest="$2"
  if ! curl -fsSL -o "$dest" "$url"; then
    err "download failed: ${url}"
  fi
  [ -s "$dest" ] || err "download empty: ${url}"
}

file_sha256() {
  f="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$f" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$f" | awk '{print $1}'
  else
    err "need sha256sum or shasum to verify download"
  fi
}

lookup_expected_sha() {
  archive_name="$1"
  sums_file="$2"
  per_asset="$3"

  if [ -f "$sums_file" ] && [ -s "$sums_file" ]; then
    line="$(grep -E "[[:space:]](\*?)${archive_name}\$" "$sums_file" | head -1 || true)"
    if [ -n "$line" ]; then
      printf '%s\n' "$line" | awk '{print $1}'
      return 0
    fi
  fi

  if [ -f "$per_asset" ] && [ -s "$per_asset" ]; then
    awk '{print $1; exit}' "$per_asset"
    return 0
  fi

  return 1
}

install_bin() {
  src="$1"
  name="$2"
  [ -f "$src" ] || return 0
  chmod +x "$src"
  dst="${INSTALL_DIR}/${name}"
  cp "$src" "${dst}.tmp"
  chmod +x "${dst}.tmp"
  mv -f "${dst}.tmp" "$dst"
}

main() {
  need_cmd curl
  need_cmd tar
  need_cmd uname
  need_cmd mktemp

  if [ -z "${BEE_INSTALL_DIR:-}" ] && [ -z "${HOME:-}" ]; then
    err "HOME is unset; set BEE_INSTALL_DIR to choose an install path"
  fi

  triple="$(detect_triple)"
  if [ -n "${BEE_VERSION:-}" ]; then
    version="$(normalize_version "$BEE_VERSION")"
  else
    printf 'Resolving latest release for %s...\n' "$REPO"
    version="$(latest_version)"
  fi
  [ -n "$version" ] || err "empty version"
  tag="v${version}"

  archive_name="bee-${version}-${triple}.tar.gz"
  base_url="${GITHUB_URL}/${REPO}/releases/download/${tag}"
  archive_url="${base_url}/${archive_name}"
  sums_url="${base_url}/SHA256SUMS"
  per_sha_url="${base_url}/${archive_name}.sha256"

  tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/bee-install.XXXXXX")"
  cleanup() { rm -rf "$tmpdir"; }
  trap cleanup EXIT INT HUP TERM

  archive_path="${tmpdir}/${archive_name}"
  sums_path="${tmpdir}/SHA256SUMS"
  per_sha_path="${tmpdir}/${archive_name}.sha256"

  printf 'Downloading %s...\n' "$archive_url"
  download "$archive_url" "$archive_path"

  curl -fsSL -o "$sums_path" "$sums_url" 2>/dev/null || true
  curl -fsSL -o "$per_sha_path" "$per_sha_url" 2>/dev/null || true

  expected="$(lookup_expected_sha "$archive_name" "$sums_path" "$per_sha_path" || true)"
  if [ -z "${expected:-}" ]; then
    err "no SHA256 checksum found for ${archive_name} (looked for SHA256SUMS and ${archive_name}.sha256 on release ${tag})"
  fi

  actual="$(file_sha256 "$archive_path")"
  if [ "$actual" != "$expected" ]; then
    err "SHA256 mismatch for ${archive_name}: expected ${expected}, got ${actual}"
  fi
  printf 'SHA256 OK (%s)\n' "$actual"

  printf 'Extracting bee...\n'
  tar xzf "$archive_path" -C "$tmpdir" || err "failed to extract ${archive_name}"
  [ -f "${tmpdir}/bee" ] || err "archive does not contain bee binary at root: ${archive_name}"

  mkdir -p "$INSTALL_DIR"
  install_bin "${tmpdir}/bee" bee
  install_bin "${tmpdir}/hivemind" hivemind
  install_bin "${tmpdir}/odm" odm

  printf 'Verifying %s --version...\n' "${INSTALL_DIR}/bee"
  if ! "${INSTALL_DIR}/bee" --version; then
    err "installed binary failed: ${INSTALL_DIR}/bee --version"
  fi

  printf '\nInstalled bee %s (%s) to %s\n' "$version" "$triple" "${INSTALL_DIR}/bee"

  case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
      printf 'Note: %s is not on PATH. Add it, e.g.:\n' "$INSTALL_DIR"
      printf '  export PATH="%s:$PATH"\n' "$INSTALL_DIR"
      ;;
  esac
}

main "$@"
