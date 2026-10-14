#!/usr/bin/env python3
"""Product adapter unit tests; these do not establish an installed QA pass."""
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('adapter', Path(__file__).with_name('installed-lifecycle-adapter.py'))
adapter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(adapter)


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


class AdapterTests(unittest.TestCase):
    def test_root_paths_and_production_callback(self):
        config = json.loads((Path(__file__).parents[1] / 'config/xcos.json.example').read_text())
        root = Path('/var/lib/xcos-test/installed')
        adapter.configure(config, root, root.parent / 'shared', 18000)
        self.assertIs(config['development'], False)
        self.assertEqual(config['bind'], '127.0.0.1:18000')
        self.assertNotEqual(config['media_callback_bind'], config['bind'])
        self.assertTrue(config['media_callback_bind'].startswith('127.0.0.1:'))
        self.assertEqual(config['data_dir'], str(root / 'state'))
        self.assertEqual(config['mediamtx_contract'], str(root / 'share/mediamtx.lock'))
        self.assertEqual(config['public_rtsp_publish_base_url'], 'rtsps://camera.example.org:8322')

    def test_imports_are_real_matching_certificates_outside_install_root(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root = base / 'installed'
            (base / 'inputs').mkdir(mode=0o700)
            files = adapter.configuration_files({}, root, base / 'inputs')
            self.assertFalse(root.exists())
            self.assertEqual(set(files), {'rtsp-cert.pem', 'rtsp-key.pem', 'mediamtx.env'})
            for path in files.values():
                self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            cert = subprocess.check_output(['openssl', 'x509', '-in', str(files['rtsp-cert.pem']), '-pubkey', '-noout'])
            key = subprocess.check_output(['openssl', 'pkey', '-in', str(files['rtsp-key.pem']), '-pubout'])
            self.assertEqual(cert, key)
            self.assertIn('MTX_RTSPSADDRESS=127.0.0.1:8322', files['mediamtx.env'].read_text())

    def test_business_restart_and_retention(self):
        client = {'id': 'id-1', 'name': 'Original lifecycle instance', 'authorization_code': 'initial-secret'}
        session = {'csrf_token': 'csrf', 'authenticated': True, 'user_id': 'admin-id'}
        calls = []
        def request(path, method, body, headers):
            calls.append((path, method))
            if method != 'GET':
                self.assertEqual(headers['X-CSRF-Token'], 'csrf')
            if body is not None:
                client.update(json.loads(body))
            value = session if path.endswith('/session') else ([client] if method == 'GET' else client)
            return 200, json.dumps(value).encode()
        sensitive = []
        state = adapter.exercise(request, session, sensitive, require)
        adapter.persisted(request, session, state, require)
        self.assertEqual(sensitive, ['initial-secret', client['authorization_code']])
        self.assertEqual(len(client['authorization_code']), 36)
        self.assertIn(('/api/v1/clients/id-1/authorization', 'PUT'), calls)
        session['authenticated'] = False
        with self.assertRaisesRegex(RuntimeError, 'session did not survive'):
            adapter.persisted(request, session, state, require)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'state/recordings').mkdir(parents=True)
            for relative in adapter.RETAINED_FILES:
                (root / relative).write_bytes(b'unit-test-only')
            adapter.retained(root, root, state, require)
            (root / 'state/recordings').rmdir()
            with self.assertRaisesRegex(RuntimeError, 'recordings directory'):
                adapter.retained(root, root, state, require)


if __name__ == '__main__':
    unittest.main()
