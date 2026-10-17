#!/usr/bin/env python3
"""Source gates for real candidate packaging, separate from installed tests."""
import importlib.util
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
CI = (ROOT / '.github/workflows/ci.yml').read_text()
SOURCE = CI.split('\n  installed_candidate:\n', 1)[0]
INSTALLED = CI.split('\n  installed_candidate:\n', 1)[1]


class CandidateWorkflow(unittest.TestCase):
    def test_added_lane_is_parallel_and_preserves_existing_checks(self):
        self.assertIn('    needs: scope\n', INSTALLED)
        self.assertNotIn('needs: source', INSTALLED)
        for command in ('cargo fmt --all -- --check',
                        'cargo check --locked --workspace --all-targets --all-features',
                        'cargo clippy --locked --workspace --all-targets --all-features -- -D warnings',
                        'cargo test --locked --workspace --all-features',
                        'npx playwright install --with-deps chromium firefox', 'npm run test:browser',
                        'python3 scripts/companion-smoke-test.py',
                        'cargo test --locked --all-features --test runtime_smoke -- --ignored --nocapture',
                        'npm run check:xcss --prefix web', 'npm run test:unit --prefix web',
                        'verify-manifest --product-root', './scripts/check-deployment.sh'):
            with self.subTest(command=command):
                self.assertIn(command, SOURCE)

    def test_one_real_release_build_feeds_pinned_producer_and_shared_lifecycle(self):
        self.assertEqual(INSTALLED.count('tools/web-build/Cargo.toml -- server --product-root . --config'), 1)
        self.assertIn('--mode release --source-revision "$GITHUB_SHA"', INSTALLED)
        self.assertIn('XSSI_BUILD_METADATA_DIR: ${{ runner.temp }}/install-metadata', INSTALLED)
        self.assertIn('XSSI_FRONTEND_METADATA_FILE: ${{ runner.temp }}/install-metadata/frontend.json', INSTALLED)
        self.assertIn('scripts/package-installed-candidate.sh', INSTALLED)
        self.assertIn('"$CARGO_TARGET_DIR/$SERVER_TARGET/release/xcos"', INSTALLED)
        self.assertIn('git -C "$installer" fetch --quiet --depth=1 origin "$revision"', INSTALLED)
        self.assertIn('python3 -I -B "$installer/tests/installed_lifecycle.py"', INSTALLED)
        self.assertIn('--adapter "$GITHUB_WORKSPACE/scripts/installed-lifecycle-adapter.py" "${candidates[0]}" "$GITHUB_SHA"', INSTALLED)
        self.assertIn('sudo env GITHUB_ACTIONS="$GITHUB_ACTIONS" RUNNER_ENVIRONMENT="$RUNNER_ENVIRONMENT"', INSTALLED)
        self.assertNotIn('package-release.py', INSTALLED)
        self.assertNotIn('git tag', INSTALLED)

    def test_precise_branch_scoped_caches_do_not_replace_real_builds_or_audits(self):
        for lane, cache in ((INSTALLED, 'release_cache'),):
            self.assertIn('rust-1.99.0', lane)
            self.assertIn("'Cargo.lock'", lane)
            self.assertIn("'tools/web-build/Cargo.lock'", lane)
            self.assertIn("'web/rust-web.toml'", lane)
            self.assertIn("'config/xssi.lock'", lane)
            self.assertIn('fingerprint', lane)
            self.assertIn('cargo clean --package xcos', lane)
            self.assertNotIn('restore-keys:', lane)
            self.assertIn("github.event_name == 'push' && github.ref == 'refs/heads/main'", lane)
            self.assertIn('steps.' + cache + '.outputs.cache-primary-key', lane)
        for version in ('cargo-audit --version 0.22.2', 'cargo-cyclonedx --version 0.5.9', 'node-version: 26.7.0'):
            self.assertIn(version, INSTALLED)
        self.assertIn("cargo audit --version | grep -Fx 'cargo-audit-audit 0.22.2'", INSTALLED)
        self.assertIn("cargo cyclonedx --version | grep -Fx 'cargo-cyclonedx-cyclonedx 0.5.9'", INSTALLED)
        self.assertIn('sha256sum --check SHA256SUMS', INSTALLED)
        self.assertIn('if: ${{ always() }}', INSTALLED)
        self.assertIn('path: ${{ runner.temp }}/install-diagnostics', INSTALLED)

    def test_media_tools_stay_external_exact_and_main_saved(self):
        script = (ROOT / 'scripts/prepare-installed-media-tools.sh').read_text()
        self.assertIn('XCOS_MEDIA_TOOLS="$RUNNER_TEMP/install-media-tools"', INSTALLED)
        self.assertIn('steps.media_environment.outputs.fingerprint', INSTALLED)
        self.assertIn('steps.media_cache.outputs.cache-primary-key', INSTALLED)
        self.assertNotIn('restore-keys:', INSTALLED)
        self.assertIn('https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz', script)
        self.assertIn('8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e', script)
        self.assertIn('a25897fd13ef46df7c6963a2567b4f320beb9461', script)
        self.assertIn('1dbdf9dc523ea62f20e44825ac9012744934435f78f75fcfad4df75cbb7544b9', script)
        for flag in ('--enable-openssl', '--disable-autodetect', '--disable-encoders',
                     '--disable-avdevice', '--disable-hwaccels', '--enable-decoder=h264'):
            self.assertIn(flag, script)
        for name in ('scripts/stage-product.sh', 'scripts/package-installed-candidate.sh',
                     'scripts/package-release.sh'):
            self.assertNotIn('XCOS_MEDIA_TOOLS', (ROOT / name).read_text())

    def test_formal_release_still_requires_current_annotated_tag(self):
        text = (ROOT / 'scripts/package-release.sh').read_text()
        self.assertIn('cat-file -t "refs/tags/v$version"', text)
        self.assertIn('A source-bound annotated release tag is required.', text)
        self.assertIn('source "$source_root/scripts/stage-product.sh"', text)
        self.assertIn('stage_xcos_payload "$source_root" "$work" "$server_binary" "$source_revision"', text)

    def test_workflow_actions_are_exact_allowlisted_references(self):
        import re
        allowed = {
            'actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1',
            'actions/setup-node@820762786026740c76f36085b0efc47a31fe5020',
            'actions/cache/restore@0400d5f644dc74513175e3cd8d07132dd4860809',
            'actions/cache/save@0400d5f644dc74513175e3cd8d07132dd4860809',
            'actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a',
        }
        references = re.findall(r'^\s*uses:\s*(\S+)', CI, re.M)
        self.assertTrue(references)
        self.assertTrue(set(references) <= allowed)
        self.assertNotIn('persist-credentials: true', CI)
        self.assertIn('permissions: {}', CI)

    def test_caller_rejects_wrong_or_dirty_installer_before_execution(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'source'
            (source / 'scripts').mkdir(parents=True)
            (source / 'config').mkdir()
            caller = source / 'scripts/package-installed-candidate.sh'
            shutil.copyfile(ROOT / 'scripts/package-installed-candidate.sh', caller)
            installer = root / 'installer'
            (installer / 'scripts').mkdir(parents=True)
            marker = root / 'producer-was-executed'
            producer = installer / 'scripts/package-installed-candidate.py'
            producer.write_text('from pathlib import Path\nPath(' + repr(str(marker)) + ').touch()\n')
            subprocess.run(['git', 'init', '--quiet', str(installer)], check=True)
            subprocess.run(['git', '-C', str(installer), 'add', '.'], check=True)
            subprocess.run(['git', '-C', str(installer), '-c', 'user.name=Fixture', '-c',
                            'user.email=fixture@example.invalid', 'commit', '--quiet', '-m', 'Owned source gate fixture'], check=True)
            revision = subprocess.check_output(['git', '-C', str(installer), 'rev-parse', 'HEAD'], text=True).strip()
            lock = source / 'config/xssi.lock'
            args = ['bash', str(caller), str(root / 'binary'), str(root / 'metadata'), str(installer), str(root / 'output')]
            lock.write_text('git_rev=' + 'a' * 40 + '\n')
            self.assertNotEqual(subprocess.run(args, capture_output=True).returncode, 0)
            self.assertFalse(marker.exists())
            lock.write_text('git_rev=' + revision + '\n')
            producer.write_text(producer.read_text() + '# unreviewed modification\n')
            self.assertNotEqual(subprocess.run(args, capture_output=True).returncode, 0)
            self.assertFalse(marker.exists())


if __name__ == '__main__':
    unittest.main()
