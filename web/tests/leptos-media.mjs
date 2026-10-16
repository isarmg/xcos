// Exercises the compiled Rust player. The archived React harness is only a
// historical regression reference; this suite runs the real WASM application.
import assert from 'node:assert/strict';
import { chromium, firefox, expect } from '@playwright/test';
import { preview } from 'vite';

const id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa';
const at = '2026-10-16T00:00:00Z';
const session = { authenticated: true, user_id: 'A'.repeat(43), username: 'admin', role: 'admin', csrf_token: 'A'.repeat(43) };
const client = { id, installation_id: id, name: 'Media fixture', client_version: '1.0.0', authorization_code: 'a'.repeat(36), status: 'online', last_seen_at: at, created_at: at, updated_at: at };
const camera = { id, name: 'Media fixture', location: '', has_sub_stream: false, source_kind: 'client', client_id: id, adapter_kind: 'onvif', manufacturer: null, model: null, firmware_version: null, serial_number: null, capabilities: { video: 'supported', main_stream: 'supported', sub_stream: 'unsupported', local_recording: 'unsupported', server_recording: 'supported', ptz: 'unsupported', events: 'unsupported', audio_input: 'unsupported', audio_output: 'unsupported' }, streams: [], health_message: null, device_status: 'online', storage_mode: 'server', enabled: true, record_enabled: true, status: 'online', observation_status: 'fresh', last_observed_at: at, observation_expires_at: '2026-10-16T00:01:00Z', observation_remaining_ms: 60_000, last_seen_at: at, created_at: at, updated_at: at };
const server = await preview({ preview: { host: '127.0.0.1', port: 0, strictPort: true } });
const address = server.httpServer.address();
assert.ok(address && typeof address === 'object');
const origin = `http://127.0.0.1:${address.port}`;

try {
  for (const engine of [chromium, firefox]) {
    const browser = await engine.launch();
    try {
      const page = await browser.newPage({ locale: 'zh-CN' });
      const errors = [], media = [], foreign = [], wasm = [];
      let tickets = 0, mode = 'whep', cameraVisible = true;
      page.on('pageerror', error => errors.push(error.message));
      page.on('request', request => { if (new URL(request.url()).pathname.endsWith('.wasm')) wasm.push(request.url()); });
      await page.addInitScript(() => {
        window.mediaPeers = [];
        // The peer fixture supplies signaling only, so it cannot produce video
        // frames to settle the native playback promise. Model that browser
        // boundary while keeping all Rust transport/state assertions intact.
        HTMLMediaElement.prototype.play = function () { return Promise.resolve(); };
        const NativeSource = window.EventSource;
        window.EventSource = class extends NativeSource { constructor(...args) { super(...args); window.mediaSource = this; } };
        window.RTCPeerConnection = class {
          constructor(configuration) { this.configuration = configuration; this.iceGatheringState = 'complete'; this.connectionState = 'connected'; this.localDescription = null; this.transceivers = []; this.closed = false; window.mediaPeers.push(this); }
          addTransceiver(kind, options) { this.transceivers.push({ kind, options }); return {}; }
          createOffer() { return Promise.resolve({ type: 'offer', sdp: 'fixture-offer' }); }
          setLocalDescription(value) { this.localDescription = value; return Promise.resolve(); }
          setRemoteDescription(value) { this.remoteDescription = value; return Promise.resolve(); }
          close() { this.closed = true; this.connectionState = 'closed'; }
        };
      });
      await page.route('**/*', async route => {
        const request = route.request(), url = new URL(request.url()), path = url.pathname;
        if (url.origin !== origin) { foreign.push(request.url()); return route.abort(); }
        if (path === '/api/v1/auth/session') return route.fulfill({ json: session });
        if (path === '/api/v1/clients') return route.fulfill({ json: [client] });
        if (path === '/api/v1/cameras') return route.fulfill({ json: cameraVisible ? [camera] : [] });
        if (path === '/api/v1/system/status') return route.fulfill({ json: { service: 'xcos', version: '1.0.0', database: 'ok', media_service: 'ok', cameras: { recording_configured: 1 }, server_time: at } });
        if (path === '/api/v1/events/stream') return route.fulfill({ status: 204 });
        if (path.endsWith('/stream-ticket')) {
          tickets += 1;
          const unsafe = mode === 'foreign';
          return route.fulfill({ json: { profile: 'main', whep_url: unsafe ? 'https://foreign.invalid/whep' : '/media-webrtc/cam/whep', hls_url: unsafe ? 'https://foreign.invalid/index.m3u8' : '/media-hls/cam/index.m3u8', token: `ticket-${tickets}`, expires_at: at } });
        }
        if (path.startsWith('/media-')) {
          media.push({ path, method: request.method(), token: request.headers().authorization });
          assert.match(request.headers().authorization ?? '', /^Bearer ticket-\d+$/);
          if (request.method() === 'OPTIONS') return route.fulfill({ status: 204, headers: { Link: '<stun:lan.example>; rel="ice-server", <turn:relay.example>; rel="ice-server"; username="a,b"; credential="Secret"' } });
          if (request.method() === 'DELETE') return route.fulfill({ status: 204 });
          if (request.method() === 'POST') {
            assert.equal(request.headers()['content-type'], 'application/sdp');
            assert.equal(request.postData(), 'fixture-offer');
            return mode === 'hls' ? route.fulfill({ status: 503 }) : route.fulfill({ status: 201, contentType: 'application/sdp', headers: { Location: '/cam/whep/session' }, body: 'fixture-answer' });
          }
          if (path.endsWith('.m3u8')) return route.fulfill({ contentType: 'application/vnd.apple.mpegurl', body: '#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:1,\nsegment.ts\n#EXT-X-ENDLIST\n' });
          return route.fulfill({ status: 204 });
        }
        return route.continue();
      });
      await page.goto(`${origin}/#details`);
      await expect.poll(() => wasm.length).toBe(1);
      await expect(page.locator("html")).toHaveAttribute("data-xcss-runtime", "leptos", { timeout: 30_000 });
      await expect.poll(() => media.filter(value => value.method === 'POST').length).toBe(1);
      await expect(page.locator('.video-shell .video-state')).toHaveCount(0).catch(async error => { console.error({ state: await page.locator('.video-state').allTextContents(), errors, media, peers: await page.evaluate(() => window.mediaPeers.map(peer => ({ closed: peer.closed, state: peer.connectionState, answer: peer.remoteDescription }))) }); throw error; });
      const peer = await page.evaluate(() => ({ configuration: window.mediaPeers[0].configuration, transceivers: window.mediaPeers[0].transceivers, answer: window.mediaPeers[0].remoteDescription }));
      assert.deepEqual(peer.transceivers.map(value => [value.kind, value.options.direction]), [['video', 'recvonly'], ['audio', 'recvonly']]);
      assert.equal(peer.configuration.iceServers.length, 2);
      assert.equal(peer.configuration.iceServers[1].username, 'a,b');
      assert.equal(peer.answer.sdp, 'fixture-answer');
      await page.getByRole('button', { name: '实例列表', exact: true }).click();
      await expect.poll(() => media.some(value => value.method === 'DELETE' && value.path === '/media-webrtc/cam/whep/session')).toBe(true);
      assert.equal(await page.evaluate(() => window.mediaPeers[0].closed), true);

      mode = 'hls';
      const ticketsBeforeHls = tickets;
      await page.getByRole('button', { name: '详细信息', exact: true }).click();
      await expect.poll(() => media.some(value => value.path === '/media-hls/cam/index.m3u8')).toBe(true);
      assert.ok(tickets >= ticketsBeforeHls + 2, 'HLS receives a newly issued ticket after WHEP fails');
      assert.equal(media.findLast(value => value.path === '/media-hls/cam/index.m3u8').token, `Bearer ticket-${ticketsBeforeHls + 2}`);
      await page.getByRole('button', { name: '实例列表', exact: true }).click();

      mode = 'foreign';
      const requestsBeforeForeign = media.length;
      await page.getByRole('button', { name: '详细信息', exact: true }).click();
      await expect(page.locator('.video-shell .video-state')).toHaveText('正在重新连接');
      await page.getByRole('button', { name: '实例列表', exact: true }).click();
      assert.equal(media.length, requestsBeforeForeign, 'foreign ticket endpoints are rejected before fetch');
      mode = 'whep';
      await page.getByRole('button', { name: '详细信息', exact: true }).click();
      await page.getByRole('button', { name: '查看与控制', exact: true }).click();
      await expect(page.getByRole('dialog', { name: 'Media fixture', exact: true })).toBeVisible();
      cameraVisible = false;
      await page.evaluate(() => window.mediaSource.dispatchEvent(new Event('open')));
      await expect(page.getByRole('dialog')).toHaveCount(0);
      await expect(page.locator('.camera-card')).toHaveCount(0);
      assert.deepEqual(foreign, []);
      assert.deepEqual(errors, []);
      await page.close();
      console.log(`${engine.name()}: compiled Rust WHEP, resource cleanup, fresh HLS fallback ticket and same-origin bearer boundary passed`);
    } finally { await browser.close(); }
  }
} finally { await new Promise((resolve, reject) => server.httpServer.close(error => error ? reject(error) : resolve())); }
