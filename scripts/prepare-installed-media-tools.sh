#!/usr/bin/env bash
# Test-only tools. Never stage these files in the installed product payload.
set -euo pipefail
export LC_ALL=C

ffmpeg_version=9.0.2
ffmpeg_sha256=8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e
ffmpeg_url=https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz
fixture_sha256=1dbdf9dc523ea62f20e44825ac9012744934435f78f75fcfad4df75cbb7544b9
fixture_url=https://raw.githubusercontent.com/isarmg/xszc/a25897fd13ef46df7c6963a2567b4f320beb9461/clients/android/app/src/androidTest/assets/video-preview.mp4
script="$(realpath -- "${BASH_SOURCE[0]}")"

[[ $# == 1 ]] || { echo 'Usage: prepare-installed-media-tools.sh --fingerprint|ABSOLUTE_TOOLS_DIRECTORY' >&2; exit 2; }
[[ -n "${ImageOS:-}" && -n "${ImageVersion:-}" ]] || { echo 'Exact runner image identity is required.' >&2; exit 2; }
for command in gcc make ld pkg-config openssl sha256sum python3; do
  command -v "$command" >/dev/null
done
# The complete script binds configure flags, validation, source and fixture pins.
# OpenSSL libraries remain system-linked; never reuse across different images,
# compilers, OpenSSL builds or effective build environments.
fingerprint="$(set -e; {
  printf '%s\n' "$ImageOS" "$ImageVersion" "$ffmpeg_version" "$ffmpeg_sha256" "$fixture_sha256"
  sha256sum "$script" | cut -d ' ' -f 1
  uname -m
  gcc --version
  gcc -dumpmachine
  sha256sum "$(realpath -- "$(command -v gcc)")"
  ld --version
  make --version
  pkg-config --modversion openssl
  pkg-config --cflags --libs openssl
  openssl version -v -b -p -o -f
  env | sort | grep -E '^(CC=|CXX=|CPPFLAGS=|CFLAGS=|CXXFLAGS=|LDFLAGS=|AR=|AS=|LD=|NM=|RANLIB=|STRIP=|PKG_CONFIG[^=]*=|SOURCE_DATE_EPOCH=)' || true
} | sha256sum | cut -d ' ' -f 1)"
if [[ "$1" == --fingerprint ]]; then
  printf '%s\n' "$fingerprint"
  exit 0
fi

output="$1"
[[ "$output" == /* && "$output" != / && ! -L "$output" ]] || { echo 'A physical absolute tools directory is required.' >&2; exit 2; }
output="$(realpath -m -- "$output")"

validate_tools() {
  python3 -I -B - "$1" "$fingerprint" "$fixture_sha256" "$ffmpeg_version" <<'PY'
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

root, identity, fixture_hash, version = Path(sys.argv[1]), *sys.argv[2:]
expected = {'ffmpeg', 'ffprobe', 'video-preview.mp4', 'SHA256SUMS', 'BUILD_FINGERPRINT'}
if root.is_symlink() or not root.is_dir() or {p.name for p in root.iterdir()} != expected:
    raise SystemExit('Unexpected media-tools directory contents')
if any((root / name).is_symlink() or not (root / name).is_file() for name in expected):
    raise SystemExit('Media tools must be physical regular files')
if (root / 'BUILD_FINGERPRINT').read_text(encoding='ascii') != identity + '\n':
    raise SystemExit('Media-tools build identity differs from this exact environment')
checksums = (root / 'SHA256SUMS').read_text(encoding='ascii')
pattern = r'([0-9a-f]{64})  ffmpeg\n([0-9a-f]{64})  ffprobe\n([0-9a-f]{64})  video-preview\.mp4\n'
match = re.fullmatch(pattern, checksums)
if match is None:
    raise SystemExit('Invalid media-tools checksum manifest')
for name, digest in zip(('ffmpeg', 'ffprobe', 'video-preview.mp4'), match.groups()):
    if hashlib.sha256((root / name).read_bytes()).hexdigest() != digest:
        raise SystemExit('Media-tools checksum mismatch: ' + name)
if match[3] != fixture_hash:
    raise SystemExit('Media fixture differs from the pinned source')

def run(name, *args):
    if not os.access(root / name, os.X_OK):
        raise SystemExit('Media tool is not executable: ' + name)
    result = subprocess.run([str(root / name), *args], capture_output=True, text=True,
                            check=True, timeout=30)
    if result.stderr:
        raise SystemExit('Media tool reported an unexpected diagnostic: ' + name)
    return result.stdout

for name in ('ffmpeg', 'ffprobe'):
    if not run(name, '-v', 'quiet', '-version').startswith(name + ' version ' + version + ' '):
        raise SystemExit('Unexpected media-tool version: ' + name)
# count_frames invokes the H.264 decoder; stream metadata or packet counting is
# insufficient. This is a local fixture check, not network/TLS acceptance.
probe = json.loads(run('ffprobe', '-v', 'error', '-count_frames', '-show_entries',
                       'stream=codec_type,codec_name,width,height,pix_fmt,r_frame_rate,duration,nb_read_frames',
                       '-of', 'json', str(root / 'video-preview.mp4')))
expected_stream = {'codec_name': 'h264', 'codec_type': 'video', 'width': 320,
                   'height': 180, 'pix_fmt': 'yuv420p', 'r_frame_rate': '12/1',
                   'duration': '12.000000', 'nb_read_frames': '144'}
if probe.get('streams') != [expected_stream]:
    raise SystemExit('Pinned media fixture did not decode to the expected 144 video frames')
print('Verified FFmpeg ' + version + ', FFprobe ' + version + ' and the pinned 144-frame fixture')
PY
}

if [[ -e "$output" ]]; then
  # A bad exact-key cache fails closed; never fall back to unrelated system tools
  # or silently bless/rebuild a mismatched cached directory.
  validate_tools "$output"
  exit 0
fi

for command in curl tar install; do command -v "$command" >/dev/null; done
work="$(mktemp -d "$(dirname -- "$output")/.install-media-tools.XXXXXXXX")"
trap 'rm -rf -- "$work"' EXIT
curl --fail --location --proto '=https' --proto-redir '=https' --tlsv1.2 --retry 3 \
  --output "$work/ffmpeg.tar.xz" "$ffmpeg_url"
printf '%s  %s\n' "$ffmpeg_sha256" "$work/ffmpeg.tar.xz" | sha256sum --check --strict -
curl --fail --location --proto '=https' --proto-redir '=https' --tlsv1.2 --retry 3 \
  --output "$work/video-preview.mp4" "$fixture_url"
printf '%s  %s\n' "$fixture_sha256" "$work/video-preview.mp4" | sha256sum --check --strict -
tar --extract --xz --file "$work/ffmpeg.tar.xz" --directory "$work" --no-same-owner
mkdir "$work/build" "$work/tools"
(
  cd "$work/build"
  # Same minimal feature set as the separately verified feasibility build.
  # Configure enables required CLI filters and RTSP transitive dependencies.
  "../ffmpeg-$ffmpeg_version/configure" --prefix="$work/install" --cc=gcc \
    --disable-autodetect --disable-everything --disable-doc --disable-debug \
    --disable-stripping --disable-x86asm --disable-programs \
    --enable-ffmpeg --enable-ffprobe --disable-avdevice --disable-hwaccels \
    --disable-encoders --enable-avfilter --enable-openssl --enable-network \
    --enable-demuxer=mov,rtsp --enable-muxer=rtsp --enable-decoder=h264 \
    --enable-parser=h264 --enable-protocol=file,pipe,tcp,tls \
    --disable-shared --enable-static
  make -j2 ffmpeg ffprobe
)
install -m 0755 "$work/build/ffmpeg" "$work/build/ffprobe" "$work/tools/"
install -m 0644 "$work/video-preview.mp4" "$work/tools/"
printf '%s\n' "$fingerprint" > "$work/tools/BUILD_FINGERPRINT"
(cd "$work/tools" && sha256sum ffmpeg ffprobe video-preview.mp4 > SHA256SUMS)
validate_tools "$work/tools"
[[ ! -e "$output" && ! -L "$output" ]]
mv -T -- "$work/tools" "$output"
