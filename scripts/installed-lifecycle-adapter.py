"""XCOS assertions for real installed QA; requires configuration import support.

Synthetic video uses the real paired-client API, verified RTSPS and recording playback.
No private database fixtures or installed-config writes.
The harness must call configuration_files before the real installer, and pass
its mapping as --configuration-file REL=SOURCE.
"""
import ipaddress
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import tempfile
import time
import uuid
from urllib.parse import parse_qs, urlencode, urlsplit

PRODUCT = 'xcos'
SERVICES = ('xcos.service', 'xcos-mediamtx.service')
ACCOUNT = 'isarmg-xcos'
RETAINED_FILES = ('state/app.sqlite3', 'state/secrets.json')
MEDIA = {}


def private_runner_ip():
    interfaces = json.loads(subprocess.check_output(['ip', '-j', '-4', 'address', 'show'], timeout=10))
    networks = [ipaddress.ip_network(value) for value in ('10.0.0.0/8', '172.16.0.0/12', '192.168.0.0/16')]
    for interface in interfaces:
        for address in interface.get('addr_info', []):
            ip = ipaddress.ip_address(address['local'])
            if address.get('scope') == 'global' and any(ip in network for network in networks):
                return str(ip)
    raise RuntimeError('Installed media QA requires an already assigned private IPv4 address')


def configure(config, root, shared, port):
    host = private_runner_ip()
    MEDIA.update(host=host, root=root)
    while True:
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            callback_port = listener.getsockname()[1]
        if callback_port != port:
            break
    config.update(bind=f'127.0.0.1:{port}', development=False,
                  data_dir=str(root / 'state'),
                  recordings_directory=str(root / 'state/recordings'),
                  public_rtsp_publish_base_url=f'rtsps://{host}:8322',
                  media_callback_bind=f'127.0.0.1:{callback_port}',
                  mediamtx_config=str(root / 'config/mediamtx.yml'),
                  mediamtx_contract=str(root / 'share/mediamtx.lock'),
                  mediamtx_binary=str(root / 'bin/mediamtx'))


def configuration_files(config, root, inputs):
    """Prepare private installer inputs, never write inside the installation."""
    host = MEDIA['host']
    certificate, key = inputs / 'rtsp-cert.pem', inputs / 'rtsp-key.pem'
    subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes',
                    '-days', '1', '-subj', '/CN=' + host,
                    '-addext', 'subjectAltName=IP:' + host,
                    '-keyout', str(key), '-out', str(certificate)],
                   check=True, timeout=30, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    key.chmod(0o600)
    certificate.chmod(0o600)
    environment = inputs / 'mediamtx.env'
    environment.write_text(
        f'MTX_WEBRTCADDITIONALHOSTS={host}\n'
        f'MTX_RTSPENCRYPTION=strict\nMTX_RTSPSADDRESS={host}:8322\n'
        f'MTX_RTSPSERVERCERT="{root}/config/rtsp-cert.pem"\n'
        f'MTX_RTSPSERVERKEY="{root}/config/rtsp-key.pem"\n'
        'MTX_PATHDEFAULTS_RECORDDELETEAFTER=168h\n')
    environment.chmod(0o600)
    MEDIA['ca'] = certificate
    return {path.name: path for path in (certificate, key, environment)}


def api(request, path, method='GET', body=None, csrf=None, token=None):
    headers = {'Content-Type': 'application/json'}
    if token:
        headers['Authorization'] = 'Bearer ' + token
    if csrf:
        headers['X-CSRF-Token'] = csrf
    status, response = request(path, method, None if body is None else json.dumps(body).encode(), headers)
    if status not in (200, 201):
        raise RuntimeError('Unexpected business API status')
    return json.loads(response)


def exercise(request, session, sensitive, require):
    csrf = session['csrf_token']
    client = api(request, '/api/v1/clients', 'POST', {'name': 'Original lifecycle instance'}, csrf)
    sensitive.append(client['authorization_code'])
    require(client['name'] == 'Original lifecycle instance', 'Instance creation differs')
    path = '/api/v1/clients/' + client['id']
    api(request, path, 'PATCH', {'name': 'Renamed lifecycle instance'}, csrf)
    code = secrets.token_hex(18)
    sensitive.append(code)
    api(request, path + '/authorization', 'PUT', {'authorization_code': code}, csrf)
    clients = api(request, '/api/v1/clients')
    require(len(clients) == 1 and clients[0]['id'] == client['id']
            and clients[0]['name'] == 'Renamed lifecycle instance'
            and clients[0]['authorization_code'] == code, 'Instance mutation differs')
    media = exercise_media(request, client['id'], code, sensitive, require)
    return {'clients': clients, 'media': media}


def persisted(request, session, state, require):
    current = api(request, '/api/v1/auth/session')
    require(current['authenticated'] is True and current['user_id'] == session['user_id'],
            'Administrator session did not survive restart')
    stable = lambda clients: [(c['id'], c['name'], c['authorization_code']) for c in clients]
    require(stable(api(request, '/api/v1/clients')) == stable(state['clients']),
            'Instances and authorization did not survive restart')
    replay_recording(request, state['media'], require)
    print('PASS: authenticated synthetic recording decoded after both-service restart', flush=True)
    register_retained_recordings(require)


def retained(root, shared, state, require):
    require(all((root / name).is_file() for name in RETAINED_FILES),
            'Operator uninstall did not retain the database and instance secrets')
    require((root / 'state/recordings').is_dir(),
            'Operator uninstall did not retain the recordings directory')


def media_command(arguments, timeout=45):
    """Never expose credential-bearing commands or captured media-tool stderr."""
    try:
        return subprocess.run(arguments, capture_output=True, timeout=timeout, check=False)
    except (OSError, subprocess.TimeoutExpired):
        raise RuntimeError('Media acceptance tool failed or exceeded its deadline') from None


def decode_video(path, require):
    result = media_command([str(Path(os.environ['XCOS_MEDIA_TOOLS']) / 'ffprobe'),
                            '-v', 'error', '-count_frames', '-show_streams',
                            '-of', 'json', str(path)])
    require(result.returncode == 0 and not result.stderr, 'Recorded video decode failed')
    streams = json.loads(result.stdout)['streams']
    require(len(streams) == 1 and streams[0]['codec_name'] == 'h264'
            and streams[0]['width'] == 320 and streams[0]['height'] == 180
            and int(streams[0].get('nb_read_frames', 0)) >= 120,
            'Recorded video did not decode the expected synthetic frames')


def snapshot(camera_id):
    return {'protocol': 'xcos-edge-v1', 'command_capacity': 0, 'command_results': [],
            'cameras': [{'id': camera_id, 'name': 'Synthetic lifecycle camera',
                         'adapter_kind': 'synthetic', 'identity': {},
                         'capabilities': {key: ('supported' if key in
                             ('video', 'main_stream', 'server_recording') else 'unsupported')
                             for key in ('video', 'main_stream', 'sub_stream', 'local_recording',
                                         'server_recording', 'ptz', 'events', 'audio_input', 'audio_output')},
                         'streams': [{'profile': 'main', 'video_codec': 'H264',
                                      'audio_codec': None, 'width': 320, 'height': 180, 'frame_rate': 12.0}],
                         'storage_mode': 'server', 'status': 'online'}]}


def exercise_media(request, camera_id, code, sensitive, require):
    paired = api(request, '/api/v1/client/pair', 'POST', {
        'protocol': 'xcos-edge-v1', 'product': 'xcos', 'installation_id': str(uuid.uuid4()),
        'name': 'Synthetic lifecycle client', 'client_version': 'installed-qa',
        'authorization_code': code})
    token = paired['access_token']
    sensitive.append(token)
    require(paired['client_id'] == camera_id and paired['protocol'] == 'xcos-edge-v1',
            'Paired client identity differs')
    deadline = time.monotonic() + 60
    publish_url = None
    while time.monotonic() < deadline:
        response = api(request, '/api/v1/client/snapshot', 'PUT', snapshot(camera_id), token=token)
        for grant in response['publish']:
            sensitive.append(grant['publish_url'])
            sensitive.extend(parse_qs(urlsplit(grant['publish_url']).query).get('jwt', []))
            if grant['camera_id'] == camera_id and grant['profile'] == 'main':
                publish_url = grant['publish_url']
        if publish_url:
            break
        time.sleep(1)
    require(publish_url is not None, 'Camera publish reconciliation did not complete')
    url = urlsplit(publish_url)
    require(url.scheme == 'rtsps' and url.hostname == MEDIA['host'] and url.port == 8322
            and url.path == '/cam_' + uuid.UUID(camera_id).hex + '_main'
            and bool(parse_qs(url.query).get('jwt')), 'Server publish grant is not verified RTSPS')
    tools = Path(os.environ['XCOS_MEDIA_TOOLS'])
    fixture = tools / 'video-preview.mp4'
    decode_video(fixture, require)
    command = [str(tools / 'ffmpeg'), '-nostdin', '-hide_banner', '-loglevel', 'error',
               '-re', '-i', str(fixture), '-map', '0:v:0', '-an', '-c:v', 'copy',
               '-f', 'rtsp', '-rtsp_transport', 'tcp', '-tls_verify', '1',
               '-verifyhost', MEDIA['host'], '-ca_file']
    with tempfile.TemporaryDirectory(prefix='xcos-media-') as temporary:
        wrong_ca, wrong_key = Path(temporary) / 'wrong.pem', Path(temporary) / 'wrong.key'
        generated = media_command(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes',
                                   '-days', '1', '-subj', '/CN=Untrusted QA CA',
                                   '-keyout', str(wrong_key), '-out', str(wrong_ca)])
        require(generated.returncode == 0, 'Unable to prepare negative TLS control')
        rejected = media_command(command + [str(wrong_ca), publish_url])
        require(rejected.returncode != 0 and b'certificate verify failed' in rejected.stderr.lower(),
                'RTSPS wrong-CA control did not explicitly reject the certificate')
    print('PASS: RTSPS wrong-CA certificate rejected', flush=True)
    published = media_command(command + [str(MEDIA['ca']), publish_url])
    require(published.returncode == 0, 'Authenticated synthetic RTSPS publication failed')
    print('PASS: authenticated synthetic clip published over verified RTSPS', flush=True)
    deadline = time.monotonic() + 30
    state = {'camera_id': camera_id}
    while time.monotonic() < deadline:
        recordings = api(request, '/api/v1/recordings?' + urlencode({'camera_id': camera_id}))
        if len(recordings) == 1 and recordings[0]['duration'] >= 10:
            state.update(start=recordings[0]['start'], duration=min(recordings[0]['duration'], 12))
            break
        time.sleep(1)
    require('start' in state, 'Completed synthetic recording was not listed')
    replay_recording(request, state, require)
    print('PASS: authenticated synthetic recording listed and decoded', flush=True)
    return state


def replay_recording(request, state, require):
    recordings = api(request, '/api/v1/recordings?' + urlencode({'camera_id': state['camera_id']}))
    require(any(row['start'] == state['start'] and row['duration'] >= state['duration']
                for row in recordings), 'Synthetic recording did not survive restart')
    status, body = request('/api/v1/recordings/play?' + urlencode({**state, 'format': 'mp4'}))
    require(status == 200 and 0 < len(body) < 256 * 1024, 'Recording playback size or status differs')
    with tempfile.TemporaryDirectory(prefix='xcos-replay-') as temporary:
        replay = Path(temporary) / 'replay.mp4'
        replay.write_bytes(body)
        replay.chmod(0o600)
        decode_video(replay, require)


def register_retained_recordings(require):
    # The existing harness hashes RETAINED_FILES only AFTER both services stop,
    # then verifies their bytes after operator uninstall. No new shared hook.
    global RETAINED_FILES
    root = MEDIA['root']
    recordings = sorted((root / 'state/recordings').rglob('*.mp4'))
    require(bool(recordings), 'Synthetic recording files are missing before retention check')
    RETAINED_FILES = tuple(dict.fromkeys((*RETAINED_FILES,
        *(str(path.relative_to(root)) for path in recordings))))
