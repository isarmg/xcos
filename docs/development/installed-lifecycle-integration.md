# Installed lifecycle QA

The `Installed release lifecycle` CI job builds one source-bound release server,
packages that exact binary with the pinned XSSI installer, then exercises a fresh
installation on a disposable GitHub-hosted systemd runner. It runs alongside the
existing source, browser, companion and deployment checks.

The test logs in over loopback TLS in production mode, creates and renames a
client instance, and changes its authorization code. It then pairs a fresh
synthetic client through the public API, submits a server-recorded H.264 camera
snapshot and waits for the real reconciler's publish grant. The runner's existing
private IPv4 address is used for the advertised RTSPS endpoint and an imported
IP-SAN certificate. No hosts, routes, firewall or system CA changes are made.

The test explicitly requires the same publication to fail certificate verification
with an unrelated CA, then publishes one complete 12-second synthetic clip over
verified RTSPS using the server-issued grant. It lists recordings through the
administrator API, downloads an MP4 smaller than 256 KiB, and independently
decodes at least 120 H.264 frames at 320×180. Both services are restarted, and the
same administrator cookie must still list and decode the recording. Stable client
identity, name and authorization code must persist; pairing/status timestamps are
allowed to change. Operator removal retains the database, instance secrets and
recordings directory. Actual recording paths join the existing retained-file list;
the shared harness hashes their bytes after both services stop and compares them
after operator uninstall. This does not establish physical-camera, live-preview,
browser-player, audio or reconnect coverage.

## Run local contract checks

```sh
python3 -I -B scripts/test-installed-lifecycle-adapter.py
python3 -I -B scripts/test-stage-product.py
python3 -I -B scripts/test-installed-candidate-workflow.py
bash -n scripts/package-installed-candidate.sh scripts/stage-product.sh scripts/package-release.sh scripts/prepare-installed-media-tools.sh
```

These tests check wiring and staging contracts; installed QA requires the real CI
runner. The adapter's certificate test uses OpenSSL. Mocked media plumbing tests are not
media acceptance evidence. The actual decode helper can also be checked against
the pinned fixture with the prepared FFprobe binary, without installing services.

## Acceptance-only media tools

`scripts/prepare-installed-media-tools.sh` builds a minimal official FFmpeg 9.0.2
CLI (OpenSSL, RTSP, MOV and H.264 decode; no encoders/devices/hardware acceleration)
and fetches the small synthetic bars/timestamp fixture from an exact public XSZC
commit. Both downloads are SHA-256 pinned. Tools remain outside the candidate and
release payloads. The exact cache includes source/fixture identity, script/flags,
runner image, compiler and OpenSSL; only main pushes save it and no fallback key
is used. The lifecycle command explicitly passes `XCOS_MEDIA_TOOLS` through sudo.

The feasibility build took about 117 seconds for configure plus make on one local
machine, excluding downloads. Cold installed CI will therefore cost more than the
previous roughly ten-minute lane. Measure both cold and cached real runs; do not
claim a ten-minute result before observing it.

## Package an existing candidate

```sh
bash scripts/package-installed-candidate.sh \
  /absolute/server/binary /absolute/build/metadata \
  /absolute/pinned/xssi /absolute/new/output
```

Use the exact clean product and pinned XSSI checkouts. The producer must support
XCOS; the lifecycle harness must support the adapter's configuration imports.
The candidate path does not create release tags or publish releases. Formal
releases still require the annotated source-bound tag.

XCOS packaging shares `scripts/stage-product.sh` with the formal packager:

```sh
source scripts/stage-product.sh
stage_xcos_payload "$source_root" "$work" "$server_binary" "$source_revision"
```

Run under `bash -euo pipefail` with absolute paths and a fresh private work
directory containing the original `metadata/`. The function copies and checks
the receipt-bound server, stages product inputs and licenses, and downloads and
verifies the pinned MediaMTX binary and original vendor license. Output is
`$work/stage`; keep `$work/media.tar.gz` and pass it as `XSSI_MEDIA_ARCHIVE` to the
normal XSSI package producer. A Python caller must set that environment variable
explicitly after the staging subprocess returns.

## Acceptance evidence

Require the installed job to verify the candidate archive and manifest, complete
the real installer with services initially stopped, start and restart both
`xcos.service` and `xcos-mediamtx.service`, pass API/session persistence and an
offline state check, then remove only its recorded service authority and verify
retained data. Imported TLS key/certificate/environment and managed YAML must be
root-owned, service-group-readable files with mode 0640 under config mode 0750.
Only redacted lifecycle diagnostics are uploaded; keys, cookies, passwords and
state files are not artifacts. Measure cold and cached CI durations separately.
