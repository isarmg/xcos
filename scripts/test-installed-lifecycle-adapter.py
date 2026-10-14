#!/usr/bin/env python3
"""Product adapter unit tests; these do not establish an installed QA pass."""
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('adapter', Path(__file__).with_name('installed-lifecycle-adapter.py'))
adapter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(adapter)


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


class AdapterTests(unittest.TestCase):
    def setUp(self):
        # Mocked plumbing must never emit installed acceptance PASS labels.
        printed = patch.object(adapter, 'print', create=True)
        printed.start()
        self.addCleanup(printed.stop)
        adapter.RETAINED_FILES = ('state/app.sqlite3', 'state/secrets.json')
        adapter.MEDIA.clear()
        adapter.MEDIA.update(host="10.20.30.40")

    @patch.object(adapter, "private_runner_ip", return_value="10.20.30.40")
    def test_root_paths_and_production_callback(self, _ip):
        config = json.loads((Path(__file__).parents[1] / 'config/xcos.json.example').read_text())
        root = Path('/var/lib/xcos-test/installed')
        adapter.configure(config, root, root.parent / 'shared', 18000)
        self.assertIs(config['development'], False)
        self.assertEqual(config['bind'], '127.0.0.1:18000')
        self.assertNotEqual(config['media_callback_bind'], config['bind'])
        self.assertTrue(config['media_callback_bind'].startswith('127.0.0.1:'))
        self.assertEqual(config['data_dir'], str(root / 'state'))
        self.assertEqual(config['mediamtx_contract'], str(root / 'share/mediamtx.lock'))
        self.assertEqual(config['public_rtsp_publish_base_url'], 'rtsps://10.20.30.40:8322')

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
            text = subprocess.check_output(["openssl", "x509", "-in", str(files["rtsp-cert.pem"]), "-noout", "-ext", "subjectAltName"], text=True)
            self.assertIn("IP Address:10.20.30.40", text)
            self.assertIn('MTX_RTSPSADDRESS=10.20.30.40:8322', files['mediamtx.env'].read_text())

    @patch.object(adapter, 'register_retained_recordings')
    @patch.object(adapter, "exercise_media", return_value={})
    @patch.object(adapter, "replay_recording")
    def test_business_restart_and_retention(self, _replay, _media, _recordings):
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
        client['status'] = 'offline'
        client['updated_at'] = 'changed-after-pairing'
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


    def test_recording_paths_join_existing_stopped_hash_retention_contract(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            adapter.MEDIA['root'] = root
            with self.assertRaisesRegex(RuntimeError, 'recording files are missing'):
                adapter.register_retained_recordings(require)
            recording = root / 'state/recordings/cam_main/segment.mp4'
            recording.parent.mkdir(parents=True)
            recording.write_bytes(b'synthetic-unit-fixture')
            adapter.register_retained_recordings(require)
            self.assertEqual(adapter.RETAINED_FILES,
                             ('state/app.sqlite3', 'state/secrets.json',
                              'state/recordings/cam_main/segment.mp4'))

    def test_private_address_requires_existing_rfc1918_interface(self):
        addresses = [{'addr_info': [{'local': '127.0.0.1', 'scope': 'host'},
                                    {'local': '203.0.113.1', 'scope': 'global'}]}]
        with patch.object(adapter.subprocess, 'check_output', return_value=json.dumps(addresses)):
            with self.assertRaisesRegex(RuntimeError, 'already assigned private'):
                adapter.private_runner_ip()
        addresses[0]['addr_info'].append({'local': '172.20.1.5', 'scope': 'global'})
        with patch.object(adapter.subprocess, 'check_output', return_value=json.dumps(addresses)):
            self.assertEqual(adapter.private_runner_ip(), '172.20.1.5')

    def test_media_command_suppresses_credentials_on_timeout(self):
        with patch.object(adapter.subprocess, 'run', side_effect=subprocess.TimeoutExpired(['secret-url'], 1)):
            with self.assertRaisesRegex(RuntimeError, '^Media acceptance tool failed or exceeded its deadline$'):
                adapter.media_command(['secret-url'])

    @patch.dict(adapter.os.environ, {'XCOS_MEDIA_TOOLS': '/unused/tools'})
    @patch.object(adapter, 'decode_video')
    @patch.object(adapter, 'media_command')
    @patch.object(adapter.time, 'sleep')
    def test_pair_grant_tls_record_and_replay_plumbing(self, _sleep, command, decode):
        adapter.MEDIA['ca'] = Path('/private/ca.pem')
        camera = '11111111-1111-4111-8111-111111111111'
        grant = 'rtsps://10.20.30.40:8322/cam_11111111111141118111111111111111_main?jwt=server-issued-secret'
        command.side_effect = [subprocess.CompletedProcess([], 0, b'', b''),
                               subprocess.CompletedProcess([], 1, b'', b'certificate verify failed'),
                               subprocess.CompletedProcess([], 0, b'', b'')]
        snapshots = 0
        def request(path, method='GET', body=None, headers=None):
            nonlocal snapshots
            if path.endswith('/pair'):
                value = json.loads(body)
                self.assertEqual(value['authorization_code'], 'rotated-code')
                self.assertEqual(value['protocol'], 'xcos-edge-v1')
                value = {'client_id': camera, 'protocol': 'xcos-edge-v1', 'access_token': 'paired-secret'}
            elif path.endswith('/snapshot'):
                snapshots += 1
                self.assertEqual(headers['Authorization'], 'Bearer paired-secret')
                self.assertEqual(json.loads(body), adapter.snapshot(camera))
                value = {'publish': [] if snapshots == 1 else [
                    {'camera_id': camera, 'profile': 'main', 'publish_url': grant}]}
            elif '/recordings/play?' in path:
                self.assertIn('format=mp4', path)
                return 200, b'synthetic mocked playback'
            else:
                value = [{'start': '2026-10-14T00:00:00Z', 'duration': 11.916}]
            return 200, json.dumps(value).encode()
        sensitive = []
        state = adapter.exercise_media(request, camera, 'rotated-code', sensitive, require)
        self.assertEqual(snapshots, 2)
        self.assertEqual(sensitive, ['paired-secret', grant, 'server-issued-secret'])
        self.assertEqual(decode.call_count, 2)
        for call in command.call_args_list[1:]:
            args = call.args[0]
            self.assertEqual(args[args.index('-tls_verify') + 1], '1')
            self.assertEqual(args[args.index('-verifyhost') + 1], '10.20.30.40')
            self.assertEqual(args[-1], grant)
        adapter.replay_recording(request, state, require)
        self.assertEqual(decode.call_count, 3)
        for code, stderr in ((0, b''), (1, b'connection refused'), (1, b'authorization failed')):
            command.side_effect = [subprocess.CompletedProcess([], 0, b'', b''),
                                   subprocess.CompletedProcess([], code, b'', stderr)]
            with self.subTest(stderr=stderr), self.assertRaisesRegex(RuntimeError, 'wrong-CA control'):
                adapter.exercise_media(request, camera, 'rotated-code', [], require)

    @patch.object(adapter, 'decode_video')
    def test_playback_rejects_oversized_or_missing_recording(self, decode):
        state = {'camera_id': 'camera', 'start': '2026-10-14T00:00:00Z', 'duration': 11.0}
        def request(path, *args):
            if '/play?' in path:
                return 200, b'x' * (256 * 1024)
            return 200, json.dumps([{'start': state['start'], 'duration': 11.0}]).encode()
        with self.assertRaisesRegex(RuntimeError, 'size or status'):
            adapter.replay_recording(request, state, require)
        decode.assert_not_called()
        with self.assertRaisesRegex(RuntimeError, 'did not survive restart'):
            adapter.replay_recording(lambda *args: (200, b'[]'), state, require)

    @patch.dict(adapter.os.environ, {'XCOS_MEDIA_TOOLS': '/unused/tools'})
    def test_decode_requires_actual_frames_and_expected_shape(self):
        stream = {'codec_name': 'h264', 'width': 320, 'height': 180, 'nb_read_frames': '144'}
        def result():
            return subprocess.CompletedProcess([], 0, json.dumps({'streams': [stream]}).encode(), b'')
        with patch.object(adapter, 'media_command', side_effect=lambda *_: result()):
            adapter.decode_video(Path('fixture.mp4'), require)
            stream['nb_read_frames'] = '0'
            with self.assertRaisesRegex(RuntimeError, 'expected synthetic frames'):
                adapter.decode_video(Path('fixture.mp4'), require)


if __name__ == '__main__':
    unittest.main()
