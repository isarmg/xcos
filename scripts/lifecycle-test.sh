#!/usr/bin/env bash
set -euo pipefail

REPOSITORY_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
# shellcheck source=/dev/null
source "$REPOSITORY_ROOT/deploy/common.sh"
TEST_ROOT="$(mktemp -d)"
cleanup() {
  chmod -R u+w -- "$TEST_ROOT" 2>/dev/null || true
  rm -rf -- "$TEST_ROOT"
}
trap cleanup EXIT
fail() { echo "deployment test failed: $*" >&2; exit 1; }

# The control command only delegates to systemd, preserving its exit status.
mkdir -p "$TEST_ROOT/bin"
cat >"$TEST_ROOT/bin/systemctl" <<'CONTROL'
#!/usr/bin/env bash
printf '%s\n' "$@" >"${XCOS_TEST_COMMAND:?}"
exit "${XCOS_TEST_EXIT:-0}"
CONTROL
chmod 0755 "$TEST_ROOT/bin/systemctl"
export XCOS_TEST_COMMAND="$TEST_ROOT/systemctl-arguments"
for action in start stop restart status enable disable; do
  PATH="$TEST_ROOT/bin:$PATH" "$REPOSITORY_ROOT/deploy/xcosctl" "$action"
  printf '%s\n' "$action" xcos.service >"$TEST_ROOT/expected"
  cmp "$TEST_ROOT/expected" "$XCOS_TEST_COMMAND" || fail "systemctl arguments changed"
done
if PATH="$TEST_ROOT/bin:$PATH" XCOS_TEST_EXIT=7 "$REPOSITORY_ROOT/deploy/xcosctl" status; then
  fail "systemctl failure was hidden"
else
  [[ "$?" == 7 ]] || fail "systemctl status code was changed"
fi
rm "$XCOS_TEST_COMMAND"
for invalid in bootstrap run --user; do
  if PATH="$TEST_ROOT/bin:$PATH" "$REPOSITORY_ROOT/deploy/xcosctl" "$invalid" >/dev/null 2>&1; then
    fail "unsupported control action accepted"
  fi
done
if PATH="$TEST_ROOT/bin:$PATH" "$REPOSITORY_ROOT/deploy/xcosctl" start arbitrary.service >/dev/null 2>&1; then
  fail "arbitrary systemctl arguments accepted"
fi
[[ ! -e "$XCOS_TEST_COMMAND" ]] || fail "invalid control request reached systemctl"

python3 - "$REPOSITORY_ROOT" <<'UNITS'
import configparser
from pathlib import Path
import sys
root = Path(sys.argv[1])
for name in ["xcos", "xcos-mediamtx"]:
    unit = configparser.ConfigParser(interpolation=None, strict=False)
    unit.read(root / "deploy" / f"{name}.service")
    service = unit["Service"]
    assert service["User"] == service["Group"] == "isarmg-xcos"
    assert service["Type"] == "simple"
    assert service["RuntimeDirectory"] == "isarmg/xcos"
    assert "StateDirectory" not in service
    assert "PrivateTmp" not in service
    assert service["ProtectHome"] == "read-only"
    assert service["RuntimeDirectoryMode"] == "0700"
    assert service["RuntimeDirectoryPreserve"] == ("yes" if name == "xcos" else "restart")
    assert service["TimeoutStopSec"] == "120s"
    assert service["UMask"] == "0077"
    assert service["NoNewPrivileges"] == "true"
    assert service["KillMode"] == "control-group"
    text = (root / "deploy" / f"{name}.service").read_text()
    assert "/current" not in text and "--release-root" not in text
    assert "verify-release" not in text and "nohup" not in text
    if name == "xcos":
        assert unit["Unit"]["Requires"] == "xcos-mediamtx.service"
        assert "xcos-mediamtx.service" in unit["Unit"]["After"]
        assert service["ExecStart"] == "/opt/isarmg/xcos/bin/xcos --config /etc/isarmg/xcos/config.json run"
        assert service["ExecStartPre"].endswith(" check")
    else:
        assert unit["Unit"]["PartOf"] == "xcos.service"
        assert " media-config > /run/isarmg/xcos/mediamtx.yml" in text
        assert service["ExecStart"] == "/opt/isarmg/xcos/bin/mediamtx /run/isarmg/xcos/mediamtx.yml"
print("systemd owner, runtime/state directories, foreground commands and companion dependency checked")
UNITS

# Render the actual installer unit transformation without touching systemd or users.
python3 - "$REPOSITORY_ROOT" "$TEST_ROOT" <<'RENDER'
import json, pathlib, subprocess, sys
root, scratch = map(pathlib.Path, sys.argv[1:])
source = (root / "install.sh").read_text().split("<<'PYUNIT'\n", 1)[1].split("\nPYUNIT", 1)[0]
settings = scratch / "custom.json"
settings.write_text(json.dumps({"data_dir": "/home/service/state %x", "recordings_directory": '/tmp/media "disk"'}))
for unit in ["xcos", "xcos-mediamtx"]:
    output = scratch / (unit + ".service")
    subprocess.run([sys.executable, "-c", source, str(settings), str(root / "deploy" / (unit + ".service")), str(output)], check=True)
    rendered = output.read_text()
    assert '"/home/service/state %%x"' in rendered
    assert '\\"disk\\"' in rendered
    assert '"/run/isarmg/xcos"' in rendered
print("custom state/recordings systemd quoting checked")
RENDER

# Exercise package layout/integrity without compiling Rust or starting services.
SOURCE_FIXTURE="$TEST_ROOT/source"
INSTALL_ROOT="$TEST_ROOT/output/xcos"
mkdir -p "$SOURCE_FIXTURE/deploy" "$SOURCE_FIXTURE/scripts" "$SOURCE_FIXTURE/config" "$TEST_ROOT/output"
cp "$REPOSITORY_ROOT/install.sh" "$REPOSITORY_ROOT/README.md" "$SOURCE_FIXTURE/"
cp "$REPOSITORY_ROOT/Cargo.toml" "$SOURCE_FIXTURE/Cargo.toml"
cp "$REPOSITORY_ROOT/deploy/common.sh" "$REPOSITORY_ROOT/deploy/xcosctl" \
  "$REPOSITORY_ROOT/deploy/xcos.service" "$REPOSITORY_ROOT/deploy/xcos-mediamtx.service" "$SOURCE_FIXTURE/deploy/"
cp "$REPOSITORY_ROOT/scripts/build.sh" "$SOURCE_FIXTURE/scripts/"
cp "$REPOSITORY_ROOT/config/mediamtx.yml" "$REPOSITORY_ROOT/config/xcos.json.example" \
  "$REPOSITORY_ROOT/config/mediamtx.env.example" "$SOURCE_FIXTURE/config/"
cat >"$TEST_ROOT/fake-mediamtx" <<'MEDIA'
#!/usr/bin/env bash
[[ "$#" == 1 && "$1" == --version ]] || exit 1
printf '%s\n' v1.20.0
MEDIA
chmod 0755 "$TEST_ROOT/fake-mediamtx"
FAKE_MEDIA_SHA="$(sha256sum "$TEST_ROOT/fake-mediamtx" | awk '{print $1}')"
printf '%s\n' version=v1.20.0 platform=linux_amd64 "sha256=$FAKE_MEDIA_SHA" >"$SOURCE_FIXTURE/config/mediamtx.lock"
export XCOS_TEST_SOURCE=0123456789abcdef0123456789abcdef01234567
XCOS_TEST_STATIC="$(printf '%s' '{"format":"deployment-fixture"}' | sha256sum | awk '{print $1}')"
export XCOS_TEST_STATIC
cat >"$TEST_ROOT/fake-xcos" <<'APP'
#!/usr/bin/env bash
set -euo pipefail
case "${1:-}" in
  --version) printf 'xcos 1.0.3 target=x86_64-unknown-linux-gnu source=%s\n' "$XCOS_TEST_SOURCE" ;;
  web-assets) printf '%s' '{"format":"deployment-fixture"}' ;;
  static-contract) printf '%s\n' "$XCOS_TEST_STATIC" ;;
  release-manifest-header) printf 'format=xcos-release-v1\nsource_revision=%s\n' "$XCOS_TEST_SOURCE" ;;
  verify-release) [[ "$(readlink -f "$0")" == "$(readlink -f "${2:?}/bin/xcos")" ]] ;;
  *) exit 2 ;;
esac
APP
chmod 0755 "$TEST_ROOT/fake-xcos"
run_build() {
  XCOS_INSTALL_ROOT="$INSTALL_ROOT" XCOS_BUILD_TARGET="$TEST_ROOT/build" \
    XCOS_MEDIAMTX_SOURCE="$TEST_ROOT/fake-mediamtx" XCOS_APP_BINARY_SOURCE="$TEST_ROOT/fake-xcos" \
    XCOS_BUILD_TEST_ROOT="$TEST_ROOT" XCOS_SOURCE_REVISION="$XCOS_TEST_SOURCE" \
    bash "$SOURCE_FIXTURE/scripts/build.sh"
}
run_build
verify_release "$INSTALL_ROOT"
[[ ! -e "$INSTALL_ROOT/releases" && ! -e "$INSTALL_ROOT/current" ]] || fail "version tree required"
for file in systemd/xcos.service systemd/xcos-mediamtx.service config/xcos.json.example config/mediamtx.env.example; do
  [[ -f "$INSTALL_ROOT/$file" ]] || fail "missing packaged $file"
done
[[ ! -e "$INSTALL_ROOT/deploy/common.sh" ]] || fail "build helper leaked into package"
manifest_before="$(sha256sum "$INSTALL_ROOT/RELEASE-MANIFEST")"
if run_build >"$TEST_ROOT/repeated-build.log" 2>&1; then
  fail "rebuild overwrote existing output"
fi
[[ "$(sha256sum "$INSTALL_ROOT/RELEASE-MANIFEST")" == "$manifest_before" ]] || fail "no-clobber rejection modified output"
chmod 0644 "$INSTALL_ROOT/config/xcos.json.example"
printf '\n' >>"$INSTALL_ROOT/config/xcos.json.example"
chmod 0444 "$INSTALL_ROOT/config/xcos.json.example"
if (verify_release "$INSTALL_ROOT") >"$TEST_ROOT/tamper.log" 2>&1; then
  fail "explicit verification accepted modified package"
fi
INSTALL_ROOT="$TEST_ROOT/output/symlink"
ln -s "$TEST_ROOT/output/xcos" "$INSTALL_ROOT"
if run_build >"$TEST_ROOT/symlink.log" 2>&1; then
  fail "build accepted a symlink destination"
fi
printf '%s\n' 'Deployment tests passed: thin systemctl forwarding, service contract, complete direct payload, no-clobber and explicit integrity checks'
