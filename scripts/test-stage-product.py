#!/usr/bin/env python3
"""Hermetic staging regressions; synthetic payloads are never installation QA."""
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

HELPER = Path(__file__).with_name('stage-product.sh').resolve()


class StageTests(unittest.TestCase):
    def prepare(self, base):
        source, work = base / 'source', base / 'work'
        (source / 'config').mkdir(parents=True)
        (work / 'metadata/contracts').mkdir(parents=True)
        for name in ('xcos.json.example', 'mediamtx.env.example', 'mediamtx.yml'):
            (source / 'config' / name).write_text(name)
        (source / 'LICENSE-APACHE').write_text('product license')
        for name in ('xcos-state-contract.json', 'xcos-web-assets.json'):
            (work / 'metadata' / name).write_text('{}')
        (work / 'metadata/contracts/install.json').write_text('{}')
        binary = base / 'server'
        binary.write_bytes(b'#!/bin/sh\nexit 0\n')
        binary.chmod(0o755)
        receipt = {'format': 'xssi-web-build-receipt-v1', 'identity': {
            'product': 'xcos', 'source_revision': 'a' * 40, 'target': 'x86_64-unknown-linux-gnu'},
            'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest()}
        Path(str(binary) + '.xssi-web-build.json').write_text(json.dumps(receipt))
        media = b'#!/bin/sh\necho v1.20.0\n'
        (source / 'config/mediamtx.lock').write_text('version=v1.20.0\nsha256=' + hashlib.sha256(media).hexdigest() + '\n')
        archive = base / 'original.tar.gz'
        with tarfile.open(archive, 'w:gz') as output:
            for name, data in [('mediamtx', media), ('LICENSE', b'vendor license')]:
                item = tarfile.TarInfo(name)
                item.size = len(data)
                output.addfile(item, io.BytesIO(data))
        (base / 'tools').mkdir()
        curl = base / 'tools/curl'
        curl.write_text('#!/bin/bash\nwhile (($#)); do if [[ "$1" == --output ]]; then cp "$TEST_ARCHIVE" "$2"; exit; fi; shift; done\nexit 1\n')
        curl.chmod(0o755)
        return source, work, binary, archive

    def stage(self, base, source, work, binary, archive):
        env = dict(os.environ, PATH=str(base / 'tools') + ':' + os.environ['PATH'], TEST_ARCHIVE=str(archive))
        return subprocess.run(['bash', '-euc', 'source "$1"; stage_xcos_payload "$2" "$3" "$4" "$5"; test "$XSSI_MEDIA_ARCHIVE" = "$3/media.tar.gz"',
                               'stage-test', str(HELPER), str(source), str(work), str(binary), 'a' * 40],
                              env=env, capture_output=True)

    def test_staged_bytes_license_and_original_archive(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source, work, binary, archive = self.prepare(base)
            result = self.stage(base, source, work, binary, archive)
            self.assertEqual(result.returncode, 0, result.stderr.decode())
            self.assertEqual((work / 'stage/bin/xcos').read_bytes(), binary.read_bytes())
            self.assertEqual((work / 'stage/share/MediaMTX-LICENSE').read_bytes(), b'vendor license')
            self.assertEqual((work / 'media.tar.gz').read_bytes(), archive.read_bytes())
            self.assertEqual((work / 'stage/share/mediamtx.lock').read_bytes(), (source / 'config/mediamtx.lock').read_bytes())

    def test_server_receipt_mismatch_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source, work, binary, archive = self.prepare(base)
            binary.write_bytes(b'replaced')
            result = self.stage(base, source, work, binary, archive)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b'replaced after verification', result.stderr)

    def test_companion_digest_mismatch_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source, work, binary, archive = self.prepare(base)
            (source / 'config/mediamtx.lock').write_text('version=v1.20.0\nsha256=' + '0' * 64 + '\n')
            result = self.stage(base, source, work, binary, archive)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse((work / 'stage/bin/mediamtx').exists())


if __name__ == '__main__':
    unittest.main()
