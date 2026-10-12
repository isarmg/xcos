#!/usr/bin/env bash

# Shared with scripts/build.sh.
# shellcheck disable=SC2034
readonly XCOS_PRODUCT="xcos"
readonly XCOS_VERSION="1.0.3"

die() {
  echo "$*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "Missing required command: $1"
}

validate_absolute_path() {
  local path="$1"
  local label="$2"
  [[ "$path" == /* ]] || die "$label must be an absolute path"
  [[ "$path" != *$'\n'* && "$path" != *$'\r'* ]] || die "$label contains a newline"
  [[ "$path" =~ ^/[A-Za-z0-9._/-]+$ ]] || die "$label contains non-portable characters"
  [[ "/$path/" != *'/../'* && "/$path/" != *'/./'* && "$path" != *'//'* ]] ||
    die "$label must be lexically normalized"
}

assert_no_symlink_components() {
  local path="$1"
  local label="$2"
  validate_absolute_path "$path" "$label"
  local current=""
  local component
  local -a components
  IFS='/' read -r -a components <<<"${path#/}"
  for component in "${components[@]}"; do
    [[ -n "$component" ]] || continue
    current="${current}/${component}"
    [[ ! -L "$current" ]] || die "$label must not traverse a symbolic link: $current"
    if [[ -e "$current" ]]; then
      [[ -d "$current" ]] || die "$label has a non-directory component: $current"
    fi
  done
}

ensure_directory() {
  local path="$1"
  local mode="$2"
  local label="$3"
  validate_absolute_path "$path" "$label"
  local current=""
  local component
  local -a components
  IFS='/' read -r -a components <<<"${path#/}"
  for component in "${components[@]}"; do
    [[ -n "$component" ]] || continue
    current="${current}/${component}"
    [[ ! -L "$current" ]] || die "$label must not traverse a symbolic link: $current"
    if [[ -e "$current" ]]; then
      [[ -d "$current" ]] || die "$label has a non-directory component: $current"
      if [[ "$mode" == 700 && "$(stat -c '%a' -- "$current")" == 700 ]]; then
        [[ "$(stat -c '%u' -- "$current")" == "$(id -u)" ]] ||
          die "$label must not traverse another owner's private directory"
      fi
    else
      mkdir -- "$current"
      chmod "$mode" -- "$current"
    fi
  done
  [[ "$(stat -c '%a' -- "$path")" == "$mode" ]] ||
    die "$label must have mode 0$mode: $path"
}

assert_directory() {
  local path="$1"
  local label="$2"
  assert_no_symlink_components "$path" "$label"
  [[ -d "$path" ]] || die "$label is missing: $path"
}

assert_private_directory() {
  local path="$1"
  local label="$2"
  assert_directory "$path" "$label"
  [[ "$(stat -c '%a' -- "$path")" == "700" ]] ||
    die "$label must have mode 0700: $path"
  [[ "$(stat -c '%u' -- "$path")" == "$(id -u)" ]] ||
    die "$label must be owned by the executing user"
}

assert_regular_file() {
  local path="$1"
  local label="$2"
  if [[ "$path" == /* ]]; then
    assert_no_symlink_components "$(dirname "$path")" "$label parent"
  fi
  [[ ! -L "$path" && -f "$path" ]] || die "$label must be a regular non-symlink file: $path"
  [[ "$(stat -c '%h' -- "$path")" == "1" ]] || die "$label must not be hard linked: $path"
}

assert_private_file() {
  local path="$1"
  local label="$2"
  assert_regular_file "$path" "$label"
  [[ "$(stat -c '%a' -- "$path")" == "600" ]] || die "$label must have mode 0600: $path"
  [[ "$(stat -c '%u' -- "$path")" == "$(id -u)" ]] ||
    die "$label must be owned by the executing user"
}

portable_relative_path() {
  local path="$1"
  [[ -n "$path" && "$path" != /* && "$path" != *'//'* ]] || return 1
  [[ "/$path/" != *'/../'* && "/$path/" != *'/./'* ]] || return 1
  [[ "$path" =~ ^[A-Za-z0-9._/-]+$ ]]
}

write_release_manifest() {
  local root="$1"
  local output="$2"
  assert_directory "$root" "release staging directory"
  assert_regular_file "$root/bin/xcos" "release Xcos binary"
  local temporary="${output}.tmp.$$"
  (
    umask 077
    {
      "$root/bin/xcos" release-manifest-header
      while IFS= read -r -d '' relative; do
        [[ "$relative" != "RELEASE-MANIFEST" ]] || continue
        portable_relative_path "$relative" || die "Release contains a non-portable path"
        local path="$root/$relative"
        [[ ! -L "$path" ]] || die "Release contains a symbolic link: $relative"
        local mode
        mode=644
        [[ -d "$path" || -x "$path" ]] && mode=755
        if [[ -d "$path" ]]; then
          echo "directory $mode $relative"
        elif [[ -f "$path" ]]; then
          [[ "$(stat -c '%h' -- "$path")" == "1" ]] ||
            die "Release contains a hard-linked file: $relative"
          local size
          local digest
          size="$(stat -c '%s' -- "$path")"
          digest="$(sha256sum -- "$path" | awk '{print $1}')"
          echo "file $mode $size $digest $relative"
        else
          die "Release contains a special file: $relative"
        fi
      done < <(find -P "$root" -mindepth 1 -printf '%P\0' | LC_ALL=C sort -z)
    } >"$temporary"
  )
  mv -T -- "$temporary" "$output"
}

verify_release() {
  local root="$1"
  assert_directory "$root" "release directory"
  local directory
  for directory in bin config systemd deploy share; do
    assert_directory "$root/$directory" "release $directory directory"
  done
  local executable
  for executable in \
    bin/xcos \
    bin/mediamtx \
    deploy/xcosctl install.sh; do
    assert_regular_file "$root/$executable" "release executable $executable"
  done
  local configuration
  for configuration in \
    config/mediamtx.yml config/mediamtx.lock \
    config/xcos.json.example config/mediamtx.env.example \
    systemd/xcos.service systemd/xcos-mediamtx.service README.md; do
    assert_regular_file "$root/$configuration" "release configuration $configuration"
  done

  local manifest="$root/RELEASE-MANIFEST"
  assert_regular_file "$manifest" "release manifest"
  local generated
  generated="$(mktemp)"
  if ! write_release_manifest "$root" "$generated"; then
    rm -f -- "$generated"
    return 1
  fi
  if ! cmp -s -- "$manifest" "$generated"; then
    rm -f -- "$generated"
    die "Release content does not match RELEASE-MANIFEST: $root"
  fi
  rm -f -- "$generated"
  "$root/bin/xcos" verify-release "$root" >/dev/null ||
    die "Release binary rejected its physical tree: $root"
}
