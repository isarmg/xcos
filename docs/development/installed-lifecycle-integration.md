# Installed lifecycle QA

The `Installed release lifecycle` CI job builds one source-bound release server,
packages that exact binary with the pinned XSSI installer, then exercises a fresh
installation on a disposable GitHub-hosted systemd runner. It runs alongside the
existing source, browser, companion and deployment checks.

The test logs in over loopback TLS in production mode, creates and renames a
client instance, changes its authorization code, restarts both services, and
checks the same session and data. Operator removal retains the database, instance
secrets and recordings directory. It does not exercise cameras or recorded video.

## Run local contract checks

```sh
python3 -I -B scripts/test-installed-lifecycle-adapter.py
python3 -I -B scripts/test-stage-product.py
python3 -I -B scripts/test-installed-candidate-workflow.py
bash -n scripts/package-installed-candidate.sh scripts/stage-product.sh scripts/package-release.sh
```

These tests check wiring and staging contracts; installed QA requires the real CI
runner. The adapter's certificate test uses OpenSSL.

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
