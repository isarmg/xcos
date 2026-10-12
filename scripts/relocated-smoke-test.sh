#!/usr/bin/env bash
set -euo pipefail

REPOSITORY_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
# shellcheck source=/dev/null
source "$REPOSITORY_ROOT/deploy/common.sh"
TEST_ROOT="$(mktemp -d)"
APP_PID=""
LEGACY_PORT_PID=""
cleanup() {
  if [[ -n "$APP_PID" ]]; then
    kill "$APP_PID" 2>/dev/null || true
    wait "$APP_PID" 2>/dev/null || true
  fi
  if [[ -n "$LEGACY_PORT_PID" ]]; then
    kill "$LEGACY_PORT_PID" 2>/dev/null || true
    wait "$LEGACY_PORT_PID" 2>/dev/null || true
  fi
  chmod -R u+w -- "$TEST_ROOT" 2>/dev/null || true
  rm -rf -- "$TEST_ROOT"
}
trap cleanup EXIT

# Hold the retired default port (or verify another process already holds it).
python3 - "$TEST_ROOT/legacy-port-ready" <<'LEGACY' &
import errno, pathlib, signal, socket, sys
listener = socket.socket()
try:
    listener.bind(('127.0.0.1', 8080))
    listener.listen()
except OSError as error:
    if error.errno != errno.EADDRINUSE:
        raise
pathlib.Path(sys.argv[1]).write_text('occupied')
signal.pause()
LEGACY
LEGACY_PORT_PID="$!"
for _ in {1..50}; do
  [[ ! -e "$TEST_ROOT/legacy-port-ready" ]] || break
  kill -0 "$LEGACY_PORT_PID" 2>/dev/null || die "Could not reserve old port"
  sleep 0.1
done
[[ -e "$TEST_ROOT/legacy-port-ready" ]] || die "Old port reservation timed out"

SMOKE_SCOPE="${XCOS_SMOKE_SCOPE:-all}"
[[ "$SMOKE_SCOPE" == all || "$SMOKE_SCOPE" == unbound ]] || die "Unknown smoke scope"
WEB_ROOT="$TEST_ROOT/build-web"
mkdir -p "$WEB_ROOT" "$TEST_ROOT/bin" "$TEST_ROOT/config"
chmod 0700 "$TEST_ROOT/config"
if [[ -n "${XCOS_SMOKE_UNBOUND_BINARY:-}" ]]; then
  BUILT_APP="$XCOS_SMOKE_UNBOUND_BINARY"
  [[ "$BUILT_APP" == /* && -f "$BUILT_APP" && -x "$BUILT_APP" && ! -L "$BUILT_APP" ]] ||
    die "Prebuilt unbound binary must be an absolute regular executable"
else
  npm ci --prefix "$REPOSITORY_ROOT/web" >/dev/null
  CARGO_PROFILE_DEV_DEBUG=0 "$REPOSITORY_ROOT/web/node_modules/.bin/xcss-build-server" \
    --config "$REPOSITORY_ROOT/xcss-web-build.json" --mode development \
    --no-install --dist "$WEB_ROOT" >"$TEST_ROOT/build.log"
  BUILT_APP="$(tail -n 1 "$TEST_ROOT/build.log")"
fi
[[ "$("$BUILT_APP" --version)" == *' source=unbound' ]] || die "Expected an unbound development build"
install -m 0555 "$BUILT_APP" "$TEST_ROOT/bin/unbound-xcos"

MEDIA_BINARY="${XCOS_SMOKE_MEDIAMTX_BINARY:-}"
if [[ -z "$MEDIA_BINARY" ]]; then
  MEDIA_VERSION="$(awk -F= '$1 == "version" {print $2}' "$REPOSITORY_ROOT/config/mediamtx.lock")"
  curl --fail --location --retry 3 --output "$TEST_ROOT/mediamtx.tar.gz" \
    "https://github.com/bluenviron/mediamtx/releases/download/$MEDIA_VERSION/mediamtx_${MEDIA_VERSION}_linux_amd64.tar.gz"
  tar -xzf "$TEST_ROOT/mediamtx.tar.gz" -C "$TEST_ROOT/bin" mediamtx
  MEDIA_BINARY="$TEST_ROOT/bin/mediamtx"
fi
[[ "$(sha256sum "$MEDIA_BINARY" | awk '{print $1}')" == "$(awk -F= '$1 == "sha256" {print $2}' "$REPOSITORY_ROOT/config/mediamtx.lock")" ]] || die "Pinned companion digest mismatch"

SCOPES=(unbound)
if [[ "$SMOKE_SCOPE" == all ]]; then
  BOUND_APP="${XCOS_SMOKE_BOUND_BINARY:-}"
  if [[ -z "$BOUND_APP" ]]; then
    "$REPOSITORY_ROOT/web/node_modules/.bin/xcss-build-server" \
      --config "$REPOSITORY_ROOT/xcss-web-build.json" --mode release \
      --no-install --rust-only --source-revision "$(git -C "$REPOSITORY_ROOT" rev-parse HEAD)" >"$TEST_ROOT/bound-build.log"
    BOUND_APP="$(tail -n 1 "$TEST_ROOT/bound-build.log")"
  fi
  [[ "$("$BOUND_APP" --version)" =~ source=[0-9a-f]{40}$ ]] || die "Expected a source-bound release build"
  install -m 0555 "$BOUND_APP" "$TEST_ROOT/bin/bound-xcos"
  SCOPES+=(bound)
fi
# Neither build output nor a release tree is needed to serve embedded assets.
mv "$WEB_ROOT" "$TEST_ROOT/hidden-build-web"
for scope in "${SCOPES[@]}"; do
  APP="$TEST_ROOT/bin/$scope-xcos"
  STATE_ROOT="$TEST_ROOT/$scope-state"
  RUNTIME_ROOT="$TEST_ROOT/$scope-runtime"
  CONFIG="$TEST_ROOT/config/$scope.json"
  read -r PORT CALLBACK_PORT < <(python3 - <<'PORTS'
import socket
listeners = [socket.socket(), socket.socket()]
for listener in listeners:
    listener.bind(('127.0.0.1', 0))
print(*(listener.getsockname()[1] for listener in listeners))
PORTS
  )
  python3 - "$CONFIG" "$STATE_ROOT" "$RUNTIME_ROOT" "$PORT" "$CALLBACK_PORT" "$REPOSITORY_ROOT" "$MEDIA_BINARY" <<'CONFIG'
import json, os, sys
path, state, runtime, port, callback, root, media = sys.argv[1:]
with open(path, 'x') as stream:
    json.dump({
        'bind': f'127.0.0.1:{port}', 'media_callback_bind': f'127.0.0.1:{callback}',
        'data_dir': state, 'development': False,
        'mediamtx_api_url': 'http://127.0.0.1:9', 'mediamtx_playback_url': 'http://127.0.0.1:9',
        'mediamtx_config': f'{root}/config/mediamtx.yml',
        'mediamtx_contract': f'{root}/config/mediamtx.lock', 'mediamtx_binary': media,
        'public_rtsp_publish_base_url': 'rtsps://xcos.example:8322',
        'status_interval_secs': 60, 'reconcile_interval_secs': 60, 'request_timeout_secs': 1,
    }, stream)
os.chmod(path, 0o600)
CONFIG
  "$APP" --config "$CONFIG" config validate >/dev/null
  "$APP" --config "$CONFIG" media-config >"$TEST_ROOT/pre-init-media.yml"
  [[ ! -e "$STATE_ROOT" ]] || die "Pure configuration commands created instance state"
  # Ordinary run must never initialize.
  if "$APP" --config "$CONFIG" run --json >"$TEST_ROOT/before-init.json" 2>"$TEST_ROOT/before-init.log"; then
    die "Run accepted missing initialization"
  fi
  [[ ! -e "$STATE_ROOT/app.sqlite3" && ! -e "$RUNTIME_ROOT" ]] || die "Rejected run created data or runtime state"
  printf '%s\n' 'relocated-smoke-bootstrap-password' |
    "$APP" --config "$CONFIG" init --username smoke-admin >"$TEST_ROOT/init.log" 2>&1 || {
      cat "$TEST_ROOT/init.log" >&2; die "Explicit initialization failed";
    }
  "$APP" --config "$CONFIG" config validate >"$TEST_ROOT/config-validate.log" 2>&1 || {
    cat "$TEST_ROOT/config-validate.log" >&2; die "Offline configuration validation failed";
  }
  [[ ! -e "$RUNTIME_ROOT" ]] || die "Init or offline validation created runtime state"
  # The same effective UID must retain a private JSON file and parent.
  chmod 0644 "$CONFIG"
  if "$APP" --config "$CONFIG" config validate >/dev/null 2>&1; then
    die "World-readable private configuration was accepted"
  fi
  chmod 0600 "$CONFIG"
  chmod 0755 "$TEST_ROOT/config"
  if "$APP" --config "$CONFIG" config validate >/dev/null 2>&1; then
    die "Non-private configuration parent was accepted"
  fi
  chmod 0700 "$TEST_ROOT/config"
  "$APP" --config "$CONFIG" media-config >"$TEST_ROOT/generated-mediamtx.yml"
  grep -Fq "http://127.0.0.1:$CALLBACK_PORT/internal/v1/media/auth" "$TEST_ROOT/generated-mediamtx.yml" || die "Callback setting did not reach generated companion config"
  grep -Fq "$STATE_ROOT/recordings/%path/" "$TEST_ROOT/generated-mediamtx.yml" || die "Recording setting did not reach generated companion config"
  [[ -f "$STATE_ROOT/secrets.json" ]] || die "Init did not create private instance keys"
  [[ $(stat -c %a "$STATE_ROOT/secrets.json") == 600 ]] || die "Instance keys are not private"
  KEY_HASH="$(sha256sum "$STATE_ROOT/secrets.json")"
  mv "$STATE_ROOT/secrets.json" "$TEST_ROOT/saved-secrets.json"
  if "$APP" --config "$CONFIG" run >/dev/null 2>&1; then die "Run accepted missing keys"; fi
  [[ ! -e "$STATE_ROOT/secrets.json" ]] || die "Run regenerated missing keys"
  mv "$TEST_ROOT/saved-secrets.json" "$STATE_ROOT/secrets.json"
  "$APP" --config "$CONFIG" check >/dev/null
  [[ "$(sha256sum "$STATE_ROOT/secrets.json")" == "$KEY_HASH" ]] || die "Check changed instance keys"
  STATIC_MANIFEST="$TEST_ROOT/$scope-web-assets.json"
  "$APP" web-assets >"$STATIC_MANIFEST"
  [[ "$("$APP" static-contract)" == "$(sha256sum "$STATIC_MANIFEST" | awk '{print $1}')" ]] || die "Embedded asset inventory differs"
  kill -0 "$LEGACY_PORT_PID" 2>/dev/null || die "Old port reservation ended before runtime check"
  APP_LOG="$TEST_ROOT/$scope.log"
  (cd /; exec "$APP" --config "$CONFIG" run) >"$APP_LOG" 2>&1 &
  APP_PID="$!"
  READY=false
  for _ in {1..120}; do
    if curl --fail --silent "http://127.0.0.1:$PORT/healthz" >/dev/null; then READY=true; break; fi
    kill -0 "$APP_PID" 2>/dev/null || break
    sleep 0.1
  done
  if [[ "$READY" != true ]]; then cat "$APP_LOG" >&2; die "Relocated binary failed to start"; fi
  CALLBACK_STATUS="$(curl --silent --show-error --output "$TEST_ROOT/callback-ready.json" --write-out '%{http_code}' "http://127.0.0.1:$CALLBACK_PORT/readyz")"
  [[ "$CALLBACK_STATUS" == 200 || "$CALLBACK_STATUS" == 503 ]] || die "Configured callback listener is unavailable"
  python3 - "$TEST_ROOT/callback-ready.json" <<'CALLBACK'
import json, sys
response = json.load(open(sys.argv[1]))
assert isinstance(response.get('ready'), bool), response
CALLBACK
python3 - "$STATIC_MANIFEST" "http://127.0.0.1:$PORT" <<'HTTP'
import hashlib,json,sys,urllib.error,urllib.request
manifest=json.load(open(sys.argv[1]))
assert manifest['format']=='web-assets-v1'
for asset in manifest['files']:
    url=sys.argv[2]+'/'+asset['path']
    with urllib.request.urlopen(url) as response:
        body=response.read()
        assert len(body)==asset['size'] and hashlib.sha256(body).hexdigest()==asset['sha256'],asset['path']
        assert response.headers['content-type']==asset['content_type'],asset['path']
        assert response.headers['x-content-type-options']=='nosniff'
        etag=response.headers['etag']
    with urllib.request.urlopen(urllib.request.Request(url,method='HEAD')) as response:
        assert not response.read() and int(response.headers['content-length'])==asset['size']
    if not asset['path'].endswith('.html'):
        try: urllib.request.urlopen(urllib.request.Request(url,headers={'If-None-Match':etag}))
        except urllib.error.HTTPError as error: assert error.code==304 and not error.read()
        else: raise AssertionError('asset did not revalidate')
try: urllib.request.urlopen(sys.argv[2]+'/assets/missing-resource.js')
except urllib.error.HTTPError as error: assert error.code==404
else: raise AssertionError('missing asset did not return 404')
HTTP
  kill "$APP_PID"
  wait "$APP_PID" 2>/dev/null || true
  APP_PID=""
  [[ "$(sha256sum "$STATE_ROOT/secrets.json")" == "$KEY_HASH" ]] || die "Run changed instance keys"
  # The release identifier has no bearing on runtime installation layout.
  if XCOS_STATIC_DIR="$TEST_ROOT/hidden-build-web" "$APP" --config "$CONFIG" run --json >"$TEST_ROOT/override.json" 2>"$TEST_ROOT/override.log"; then
    die "Production accepted external Web assets"
  fi
  python3 - "$TEST_ROOT/override.json" <<'ERROR'
import json, sys
assert json.load(open(sys.argv[1]))['code'] == 'invalid_development_override'
ERROR
  if "$APP" --json >"$TEST_ROOT/missing-command.json" 2>/dev/null; then
    die "Implicit run was accepted"
  fi
  echo "$scope binary: no-release-tree startup, explicit init, offline validation, generated media config and all embedded assets passed"
done
