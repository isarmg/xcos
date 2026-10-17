// Exercises the compiled Rust player through actual browser DOM and network boundaries.
import assert from 'node:assert/strict';
import { chromium, firefox, expect } from '@playwright/test';
import { preview } from '../../tools/web-build/test-preview.mjs';

const id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa';
const stream = `cam_${id.replaceAll('-', '')}_main`;
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
      let whepPrefix = '/media-webrtc', resourceLocation = null;
      page.on('pageerror', error => errors.push(error.message));
      page.on('request', request => { if (new URL(request.url()).pathname.endsWith('.wasm')) wasm.push(request.url()); });
      await page.addInitScript(() => {
        window.mediaPeers = [];
        const nativeFetch = window.fetch.bind(window);
        window.fetch = async (input, options = {}) => {
          const response = await nativeFetch(input, options);
          if (options.method === 'POST' && window.holdWhepResponse) {
            window.holdWhepResponse = false;
            await new Promise(resolve => { window.releaseWhepResponse = resolve; });
          }
          return response;
        };
        // The peer fixture supplies signaling only, so it cannot produce video
        // frames to settle the native playback promise. Model that browser
        // boundary while keeping all Rust transport/state assertions intact.
        HTMLMediaElement.prototype.play = function () { return Promise.resolve(); };
        const NativeSource = window.EventSource;
        window.EventSource = class extends NativeSource { constructor(...args) { super(...args); window.mediaSource = this; } };
        window.RTCPeerConnection = class {
          constructor(configuration) { this.configuration = configuration; this.iceGatheringState = 'complete'; this.connectionState = 'connected'; this.localDescription = null; this.transceivers = []; this.closed = false; this.closeCount = 0; this.stream = new MediaStream(); this.holdOffer = window.holdWhepOffer; window.holdWhepOffer = false; window.mediaPeers.push(this); }
          addTransceiver(kind, options) { this.transceivers.push({ kind, options }); return {}; }
          async createOffer() { if (this.holdOffer) await new Promise(resolve => { this.releaseOffer = resolve; }); return { type: 'offer', sdp: 'fixture-offer' }; }
          setLocalDescription(value) { this.localDescription = value; return Promise.resolve(); }
          setRemoteDescription(value) { this.remoteDescription = value; this.ontrack?.({ streams: [this.stream] }); return Promise.resolve(); }
          close() { this.closeCount += 1; this.closed = true; this.connectionState = 'closed'; }
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
          return route.fulfill({ json: { profile: 'main', whep_url: unsafe ? 'https://foreign.invalid/whep' : `${whepPrefix}/${stream}/whep`, hls_url: unsafe ? 'https://foreign.invalid/index.m3u8' : '/media-hls/cam/index.m3u8', token: `ticket-${tickets}`, expires_at: at } });
        }
        if (path.startsWith('/media-')) {
          media.push({ path, search: url.search, method: request.method(), token: request.headers().authorization });
          assert.match(request.headers().authorization ?? '', /^Bearer ticket-\d+$/);
          if (request.method() === 'OPTIONS') return route.fulfill({ status: 204, headers: { Link: '<stun:lan.example>; rel="ice-server", <turn:relay.example>; rel="ice-server"; username="a,b"; credential="Secret"' } });
          if (request.method() === 'DELETE') return route.fulfill({ status: 204 });
          if (request.method() === 'POST') {
            assert.equal(request.headers()['content-type'], 'application/sdp');
            assert.equal(request.postData(), 'fixture-offer');
            return mode === 'hls' ? route.fulfill({ status: 503 }) : route.fulfill({ status: 201, contentType: 'application/sdp', headers: { Location: resourceLocation ?? `/${stream}/whep/session-${media.filter(value => value.method === 'POST').length}` }, body: 'fixture-answer' });
          }
          if (path.endsWith('.m3u8')) return route.fulfill({ contentType: 'application/vnd.apple.mpegurl', body: '#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:1,\nsegment.ts\n#EXT-X-ENDLIST\n' });
          return route.fulfill({ status: 204 });
        }
        return route.continue();
      });
      await page.goto(`${origin}/#details`);
      await expect.poll(() => wasm.length).toBe(1);
      await expect(page.locator("html")).toHaveAttribute("data-xcss-runtime", "xcss-web", { timeout: 30_000 });
      await expect.poll(() => media.filter(value => value.method === 'POST').length).toBe(1);
      await expect(page.locator('.video-shell .video-state')).toHaveCount(0).catch(async error => { console.error({ state: await page.locator('.video-state').allTextContents(), errors, media, peers: await page.evaluate(() => window.mediaPeers.map(peer => ({ closed: peer.closed, state: peer.connectionState, answer: peer.remoteDescription }))) }); throw error; });
      const peer = await page.evaluate(() => ({ configuration: window.mediaPeers[0].configuration, transceivers: window.mediaPeers[0].transceivers, answer: window.mediaPeers[0].remoteDescription }));
      assert.deepEqual(peer.transceivers.map(value => [value.kind, value.options.direction]), [['video', 'recvonly'], ['audio', 'recvonly']]);
      assert.equal(peer.configuration.iceServers.length, 2);
      assert.equal(peer.configuration.iceServers[1].username, 'a,b');
      assert.equal(peer.answer.sdp, 'fixture-answer');
      await page.getByRole('button', { name: '实例列表', exact: true }).click();
      await expect.poll(() => media.some(value => value.method === 'DELETE' && value.path === `/media-webrtc/${stream}/whep/session-1`)).toBe(true);
      assert.equal(await page.evaluate(() => window.mediaPeers[0].closeCount), 1);
      assert.equal(media.filter(value => value.method === 'DELETE' && value.path.endsWith('/session-1')).length, 1);

      await page.evaluate(() => { window.holdWhepOffer = true; });
      const offerPeer = await page.evaluate(() => window.mediaPeers.length);
      await page.getByRole('button', { name: '详细信息', exact: true }).click();
      await expect.poll(() => page.evaluate(index => typeof window.mediaPeers[index]?.releaseOffer, offerPeer)).toBe('function');
      await page.getByRole('button', { name: '实例列表', exact: true }).click();
      await page.getByRole('button', { name: '详细信息', exact: true }).click();
      await expect.poll(() => page.evaluate(() => document.querySelector('.video-shell video')?.srcObject === window.mediaPeers.at(-1).stream)).toBe(true);
      await page.evaluate(index => window.mediaPeers[index].releaseOffer(), offerPeer);
      await expect.poll(() => page.evaluate(index => window.mediaPeers[index].closeCount, offerPeer)).toBe(1);
      assert.equal(await page.evaluate(() => document.querySelector('.video-shell video').srcObject === window.mediaPeers.at(-1).stream), true, 'a late cancelled offer cannot detach the replacement video');
      await page.getByRole('button', { name: '实例列表', exact: true }).click();

      await page.evaluate(() => { window.holdWhepResponse = true; });
      const latePeer = await page.evaluate(() => window.mediaPeers.length);
      await page.getByRole('button', { name: '详细信息', exact: true }).click();
      await expect.poll(() => page.evaluate(() => typeof window.releaseWhepResponse)).toBe('function');
      const lateResource = `/media-webrtc/${stream}/whep/session-${media.filter(value => value.method === 'POST').length}`;
      await page.getByRole('button', { name: '实例列表', exact: true }).click();
      await page.getByRole('button', { name: '详细信息', exact: true }).click();
      await expect.poll(() => page.evaluate(() => document.querySelector('.video-shell video')?.srcObject === window.mediaPeers.at(-1).stream)).toBe(true);
      await page.evaluate(() => window.releaseWhepResponse());
      await expect.poll(() => media.filter(value => value.method === 'DELETE' && value.path === lateResource).length).toBe(1);
      assert.equal(await page.evaluate(index => window.mediaPeers[index].closeCount, latePeer), 1);
      assert.equal(await page.evaluate(index => window.mediaPeers[index].ontrack == null, latePeer), true, 'closed peers release their track callback');
      assert.equal(await page.evaluate(() => document.querySelector('.video-shell video').srcObject === window.mediaPeers.at(-1).stream), true, 'late resource cleanup cannot detach the replacement video');
      await page.getByRole('button', { name: '实例列表', exact: true }).click();
      assert.equal(media.filter(value => value.method === 'DELETE' && value.path === lateResource).length, 1, 'repeated cleanup must not repeat DELETE');

      // The gateway can use any configured same-origin base, and may already
      // rewrite upstream Location headers. Exercise URL semantics in WASM.
      for (const [location, expectedPath, expectedSearch] of [
        [`/${stream}/whep/custom`, `/media-custom/rtc/${stream}/whep/custom`, ''],
        [`/media-custom/rtc/${stream}/whep/prefixed`, `/media-custom/rtc/${stream}/whep/prefixed`, ''],
        [`${origin}/${stream}/whep/absolute?keep=1`, `/media-custom/rtc/${stream}/whep/absolute`, '?keep=1'],
        ['whep/relative?keep=2', `/media-custom/rtc/${stream}/whep/relative`, '?keep=2'],
        ['?keep=3', `/media-custom/rtc/${stream}/whep`, '?keep=3'],
      ]) {
        whepPrefix = '/media-custom/rtc';
        resourceLocation = location;
        await page.getByRole('button', { name: '详细信息', exact: true }).click();
        await expect(page.locator('.video-shell .video-state')).toHaveCount(0);
        await page.getByRole('button', { name: '实例列表', exact: true }).click();
        await expect.poll(() => media.some(value => value.method === 'DELETE' && value.path === expectedPath && value.search === expectedSearch)).toBe(true);
      }
      // A Location header is untrusted even after same-origin signaling succeeds.
      for (const location of ['https://foreign.invalid/session', '//foreign.invalid/session', `${origin.replace('://', '://user:password@')}/session`]) {
        resourceLocation = location;
        const deletesBefore = media.filter(value => value.method === 'DELETE').length;
        const ticketsBefore = tickets;
        await page.getByRole('button', { name: '详细信息', exact: true }).click();
        await expect.poll(() => tickets).toBeGreaterThan(ticketsBefore + 1);
        await page.getByRole('button', { name: '实例列表', exact: true }).click();
        assert.equal(media.filter(value => value.method === 'DELETE').length, deletesBefore);
      }
      whepPrefix = '/media-webrtc';
      resourceLocation = null;

      mode = 'hls';
      const ticketsBeforeHls = tickets;
      const mediaBeforeHls = media.length;
      const fallbackToken = `Bearer ticket-${ticketsBeforeHls + 2}`;
      // Invalid Location cases above also use HLS. Neither their recorded
      // manifests nor a late request with an older ticket proves this fallback.
      const currentManifest = () => media.slice(mediaBeforeHls).find(value =>
        value.path === '/media-hls/cam/index.m3u8' && value.token === fallbackToken);
      await page.getByRole('button', { name: '详细信息', exact: true }).click();
      await expect.poll(() => currentManifest()?.token).toBe(fallbackToken);
      assert.ok(tickets >= ticketsBeforeHls + 2, 'HLS receives a newly issued ticket after WHEP fails');
      assert.equal(currentManifest().token, fallbackToken);
      assert.ok(media.slice(mediaBeforeHls).some(value => value.method === 'POST' && value.token === `Bearer ticket-${ticketsBeforeHls + 1}`), 'WHEP used the first ticket before HLS received the second');
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
