"""XCOS assertions for real installed QA; requires configuration import support.

No private database fixtures, camera simulation or installed-config writes.
The harness must call configuration_files before the real installer, and pass
its mapping as --configuration-file REL=SOURCE.
"""
import json
import secrets
import socket
import subprocess

PRODUCT = 'xcos'
SERVICES = ('xcos.service', 'xcos-mediamtx.service')
ACCOUNT = 'isarmg-xcos'
RETAINED_FILES = ('state/app.sqlite3', 'state/secrets.json')
PUBLIC_HOST = 'camera.example.org'


def configure(config, root, shared, port):
    while True:
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            callback_port = listener.getsockname()[1]
        if callback_port != port:
            break
    config.update(bind=f'127.0.0.1:{port}', development=False,
                  data_dir=str(root / 'state'),
                  recordings_directory=str(root / 'state/recordings'),
                  public_rtsp_publish_base_url=f'rtsps://{PUBLIC_HOST}:8322',
                  media_callback_bind=f'127.0.0.1:{callback_port}',
                  mediamtx_config=str(root / 'config/mediamtx.yml'),
                  mediamtx_contract=str(root / 'share/mediamtx.lock'),
                  mediamtx_binary=str(root / 'bin/mediamtx'))


def configuration_files(config, root, inputs):
    """Prepare private installer inputs, never write inside the installation."""
    certificate, key = inputs / 'rtsp-cert.pem', inputs / 'rtsp-key.pem'
    subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes',
                    '-days', '1', '-subj', '/CN=' + PUBLIC_HOST,
                    '-addext', 'subjectAltName=DNS:' + PUBLIC_HOST,
                    '-keyout', str(key), '-out', str(certificate)],
                   check=True, timeout=30, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    key.chmod(0o600)
    certificate.chmod(0o600)
    environment = inputs / 'mediamtx.env'
    environment.write_text(
        f'MTX_WEBRTCADDITIONALHOSTS={PUBLIC_HOST}\n'
        'MTX_RTSPENCRYPTION=strict\nMTX_RTSPSADDRESS=127.0.0.1:8322\n'
        f'MTX_RTSPSERVERCERT="{root}/config/rtsp-cert.pem"\n'
        f'MTX_RTSPSERVERKEY="{root}/config/rtsp-key.pem"\n'
        'MTX_PATHDEFAULTS_RECORDDELETEAFTER=168h\n')
    environment.chmod(0o600)
    return {path.name: path for path in (certificate, key, environment)}


def api(request, path, method='GET', body=None, csrf=None):
    headers = {'Content-Type': 'application/json'}
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
    return {'clients': clients}


def persisted(request, session, state, require):
    current = api(request, '/api/v1/auth/session')
    require(current['authenticated'] is True and current['user_id'] == session['user_id'],
            'Administrator session did not survive restart')
    require(api(request, '/api/v1/clients') == state['clients'],
            'Instances and authorization did not survive restart')


def retained(root, shared, state, require):
    require(all((root / name).is_file() for name in RETAINED_FILES),
            'Operator uninstall did not retain the database and instance secrets')
    require((root / 'state/recordings').is_dir(),
            'Operator uninstall did not retain the recordings directory')
