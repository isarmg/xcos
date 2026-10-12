#!/usr/bin/env bash
# Install only the current systemd layout. No legacy configuration conversion.
set -euo pipefail
usage() { echo 'Usage: install.sh setup CONFIG_JSON [USERNAME] | upgrade' >&2; exit 2; }
[[ $# -ge 1 ]] || usage
action="$1"; shift
case "$action" in
  setup) [[ $# -ge 1 && $# -le 2 ]] || usage; source_config="$1"; username="${2:-admin}" ;;
  upgrade) [[ $# == 0 ]] || usage ;;
  *) usage ;;
esac
[[ $(id -u) == 0 ]] || { echo 'Run installation as root; the service itself runs as isarmg-xcos.' >&2; exit 1; }
source_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
"$source_root/bin/xcos" verify-release "$source_root" >/dev/null
install_root=/opt/isarmg/xcos
config_root=/etc/isarmg/xcos
if [[ "$action" == setup ]]; then
  [[ -f "$source_config" && ! -L "$source_config" ]] || { echo 'A regular JSON configuration is required.' >&2; exit 1; }
  settings_source="$source_config"
else
  settings_source="$config_root/config.json"
fi
preflight="$(mktemp -d)"
trap 'rm -rf -- "$preflight"' EXIT
install -m 0600 "$settings_source" "$preflight/config.json"
env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin "$source_root/bin/xcos" --config "$preflight/config.json" config validate
state_root="$(python3 - "$settings_source" <<'PYCONFIG'
import json, os, sys
with open(sys.argv[1]) as stream:
    value = json.load(stream).get('data_dir', '/var/lib/isarmg/xcos')
if not isinstance(value, str) or not value.startswith('/') or os.path.normpath(value) != value or value == '/':
    raise SystemExit('data_dir must be a normalized absolute directory')
print(value)
PYCONFIG
)"
[[ ! -L "$install_root" && ! -L "$config_root" && ! -L "$state_root" ]] || { echo 'Installation paths must not be symbolic links.' >&2; exit 1; }
if [[ "$action" == setup ]]; then
  [[ ! -e "$config_root/config.json" && ! -e "$state_root/secrets.json" ]] || { echo 'Setup refuses existing configuration or keys. Use upgrade for an initialized instance.' >&2; exit 1; }
  [[ -f "$source_config" && ! -L "$source_config" ]] || { echo 'A regular JSON configuration is required.' >&2; exit 1; }
  if [[ -e "$state_root" ]]; then
    [[ -d "$state_root" && ! -L "$state_root" && -z "$(find "$state_root" -mindepth 1 -maxdepth 1 -print -quit)" ]] || { echo 'Setup requires a new empty data directory.' >&2; exit 1; }
  fi
  if ! getent passwd isarmg-xcos >/dev/null; then
    useradd --system --user-group --home-dir "$state_root" --no-create-home --shell /usr/sbin/nologin isarmg-xcos
  fi
fi
[[ $(id -u isarmg-xcos) != 0 ]] || { echo 'xcos must have an ordinary non-root UID.' >&2; exit 1; }
install -d -o root -g root -m 0755 /opt/isarmg /etc/isarmg /var/lib/isarmg
if [[ "$action" == upgrade ]]; then
  [[ -f "$config_root/config.json" && -f "$state_root/secrets.json" ]] || { echo 'Upgrade requires a current initialized installation.' >&2; exit 1; }
  was_active=false
  if systemctl is-active --quiet xcos.service; then was_active=true; fi
  systemctl stop xcos.service
fi
# A package can be extracted elsewhere; copy its contents, never depend on its name.
if [[ "$source_root" != "$install_root" ]]; then
  install -d -o root -g root -m 0755 "$install_root"
  for directory in bin config systemd deploy share; do
    install -d -o root -g root -m 0755 "$install_root/$directory"
    for file in "$source_root/$directory/"*; do
      [[ -f "$file" && ! -L "$file" ]] || { echo 'Unexpected package entry.' >&2; exit 1; }
      mode=0644; [[ -x "$file" ]] && mode=0755
      install -o root -g root -m "$mode" "$file" "$install_root/$directory/"
    done
  done
fi
if [[ "$action" == setup ]]; then
  install -d -o isarmg-xcos -g isarmg-xcos -m 0700 "$config_root" "$state_root"
  recordings_root="$(python3 - "$settings_source" "$state_root" <<'PYRECORDINGS'
import json, pathlib, sys
settings = json.loads(pathlib.Path(sys.argv[1]).read_text())
print(settings.get('recordings_directory') or str(pathlib.Path(sys.argv[2]) / 'recordings'))
PYRECORDINGS
)"
  [[ ! -L "$recordings_root" ]] || { echo 'Recordings directory must not be a symbolic link.' >&2; exit 1; }
  if [[ ! -e "$recordings_root" ]]; then
    install -d -o isarmg-xcos -g isarmg-xcos -m 0700 "$recordings_root"
  fi
  install -o isarmg-xcos -g isarmg-xcos -m 0600 "$source_config" "$config_root/config.json"
  install -o isarmg-xcos -g isarmg-xcos -m 0600 "$source_root/config/mediamtx.yml" "$config_root/mediamtx.yml"
  install -o isarmg-xcos -g isarmg-xcos -m 0600 "$source_root/config/mediamtx.env.example" "$config_root/mediamtx.env"
  runuser -u isarmg-xcos -- env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin "$install_root/bin/xcos" --config "$config_root/config.json" config validate
  runuser -u isarmg-xcos -- env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin "$install_root/bin/xcos" --config "$config_root/config.json" init --username "$username"
fi
for unit in xcos.service xcos-mediamtx.service; do
  python3 - "$config_root/config.json" "$install_root/systemd/$unit" "$preflight/$unit" <<'PYUNIT'
import json, pathlib, sys
settings = json.loads(pathlib.Path(sys.argv[1]).read_text())
state = settings.get('data_dir', '/var/lib/isarmg/xcos')
recordings = settings.get('recordings_directory') or str(pathlib.Path(state) / 'recordings')
def quoted(value):
    # systemd performs specifier expansion even inside quoted directive values.
    return '"' + value.replace('\\', '\\\\').replace('"', '\\"').replace('%', '%%').replace('\n', '\\n').replace('\r', '\\r').replace('\t', '\\t') + '"'
text = pathlib.Path(sys.argv[2]).read_text()
text = text.replace('ReadWritePaths=/var/lib/isarmg/xcos /run/isarmg/xcos',
                    'ReadWritePaths=' + ' '.join(quoted(p) for p in [state, recordings, '/run/isarmg/xcos']))
pathlib.Path(sys.argv[3]).write_text(text)
PYUNIT
  install -o root -g root -m 0644 "$preflight/$unit" "/etc/systemd/system/$unit"
done
systemctl daemon-reload
runuser -u isarmg-xcos -- env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin "$install_root/bin/xcos" --config "$config_root/config.json" check
if [[ "$action" == upgrade ]]; then
  if [[ "$was_active" == true ]]; then systemctl start xcos.service; fi
else
  echo 'Initialized. Configure the MediaMTX TLS/ICE environment and certificates, then systemctl enable --now xcos.service.'
fi
