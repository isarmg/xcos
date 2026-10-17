import assert from 'node:assert/strict';
import { chromium, firefox, expect } from '@playwright/test';
import { preview } from '../../tools/web-build/test-preview.mjs';

const id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa';
const at = '2026-10-16T00:00:00Z';
const session = { authenticated: true, user_id: 'A'.repeat(43), username: 'admin', role: 'admin', csrf_token: 'A'.repeat(43) };
const client = { id, installation_id: id, name: 'Concurrency fixture', client_version: '1.0.0', authorization_code: 'a'.repeat(36), status: 'online', last_seen_at: at, created_at: at, updated_at: at };
const camera = { id, name: client.name, location: '', has_sub_stream: false, source_kind: 'client', client_id: id, adapter_kind: 'onvif', manufacturer: null, model: null, firmware_version: null, serial_number: null, capabilities: { video: 'supported', main_stream: 'supported', sub_stream: 'unsupported', local_recording: 'unsupported', server_recording: 'supported', ptz: 'supported', events: 'unsupported', audio_input: 'unsupported', audio_output: 'unsupported' }, streams: [], health_message: null, device_status: 'online', storage_mode: 'server', enabled: true, record_enabled: true, status: 'online', observation_status: 'fresh', last_observed_at: at, observation_expires_at: '2026-10-16T00:01:00Z', observation_remaining_ms: 60_000, last_seen_at: at, created_at: at, updated_at: at };
const server = await preview({ preview: { host: '127.0.0.1', port: 0, strictPort: true } });
try {
  for (const engine of [chromium, firefox]) {
    const browser = await engine.launch();
    try {
      const page = await browser.newPage({ locale: 'zh-CN' });
      const errors = [], commands = [], queries = [], wasm = [];
      const snapshots = { cameras: 0, clients: 0, system: 0 };
      let holdSystem = false, releaseSystem, failCamera = false, systemActive = 0, maxSystemActive = 0;
      let releaseMove, failLookup = true;
      let currentClient = { ...client }, failClientReads = false, failRename = true, releaseRename;
      const names = [];
      page.on('pageerror', error => errors.push(error.message));
      page.on('request', request => { if (new URL(request.url()).pathname.endsWith('.wasm')) wasm.push(request.url()); });
      await page.addInitScript(() => {
        window.concurrencyNotices = [];
        window.addEventListener('xcss-notify', event => window.concurrencyNotices.push(event.detail));
        const NativeSource = window.EventSource;
        window.EventSource = class extends NativeSource {
          constructor(...args) { super(...args); window.concurrencySource = this; }
        };
      });
      await page.route('**/api/v1/**', async route => {
        const request = route.request(), path = new URL(request.url()).pathname;
        if (path.endsWith('/auth/session')) return route.fulfill({ json: session });
        if (path.endsWith('/events/stream')) return route.fulfill({ status: 204 });
        if (path.endsWith('/clients')) {
          snapshots.clients += 1;
          return failClientReads ? route.fulfill({ status: 503, json: { status: 503, code: 'platform.internal', message: 'fixture list unavailable' } }) : route.fulfill({ json: [currentClient] });
        }
        if (path === `/api/v1/clients/${id}` && request.method() === 'PATCH') {
          names.push(request.postDataJSON().name);
          if (failRename) {
            await new Promise(resolve => { releaseRename = resolve; });
            failRename = false;
            return route.fulfill({ status: 503, json: { status: 503, code: 'platform.internal', message: 'fixture write unavailable' } });
          }
          currentClient = { ...currentClient, name: names.at(-1) };
          return route.fulfill({ json: currentClient });
        }
        if (path.endsWith('/cameras')) {
          snapshots.cameras += 1;
          return failCamera ? route.fulfill({ status: 503, json: { status: 503, code: 'platform.internal', message: 'fixture read unavailable' } }) : route.fulfill({ json: [camera] });
        }
        if (path.endsWith('/system/status')) {
          snapshots.system += 1;
          systemActive += 1; maxSystemActive = Math.max(maxSystemActive, systemActive);
          if (holdSystem) await new Promise(resolve => { releaseSystem = resolve; });
          systemActive -= 1;
          return route.fulfill({ json: { service: 'xcos', version: '1.0.0', database: 'ok', media_service: 'ok', cameras: { recording_configured: 1 }, server_time: at } });
        }
        if (path.endsWith('/stream-ticket')) return route.fulfill({ status: 503, json: { status: 503, code: 'media.unavailable', message: 'fixture media unavailable' } });
        if (path.endsWith('/ptz')) {
          commands.push(request.postDataJSON());
          const command_id = `018f1f4b-7a5d-7b5f-8d31-${String(commands.length).padStart(12, '0')}`;
          if (commands.length === 1) await new Promise(resolve => { releaseMove = resolve; });
          return route.fulfill({ status: 202, json: { command_id, state: 'queued', error_code: null } });
        }
        if (path.includes('/commands/')) {
          queries.push({ path, method: request.method() });
          if (failLookup) { failLookup = false; return route.fulfill({ status: 503, json: { status: 503, code: 'platform.internal', message: 'fixture status unavailable' } }); }
          return route.fulfill({ json: { command_id: path.split('/').at(-1), state: 'succeeded', error_code: null } });
        }
        throw new Error(`Unexpected concurrency request ${request.method()} ${path}`);
      });
      await page.goto(`${server.resolvedUrls.local[0]}#instances`);
      await expect(page.locator('html')).toHaveAttribute('data-xcss-runtime', 'xcss-web', { timeout: 30_000 });
      const refresh = page.getByRole('group', { name: '全局操作' }).getByRole('button', { name: '刷新', exact: true });
      await expect(refresh).toBeEnabled();
      await expect(page.getByRole('link', { name: `选择实例 ${client.name}`, exact: true })).toBeVisible();
      const before = { ...snapshots };
      holdSystem = true; failCamera = true;
      await refresh.click();
      await expect.poll(() => typeof releaseSystem).toBe('function');
      await expect(refresh).toBeDisabled();
      await page.evaluate(() => {
        for (let index = 0; index < 3; index += 1) window.concurrencySource.dispatchEvent(new Event('open'));
      });
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      assert.deepEqual(snapshots, Object.fromEntries(Object.entries(before).map(([key, count]) => [key, count + 1])), 'a rejected loader must not release the snapshot slot before the other loaders finish');
      holdSystem = false; failCamera = false; releaseSystem();
      await expect(refresh).toBeEnabled();
      assert.deepEqual(snapshots, Object.fromEntries(Object.entries(before).map(([key, count]) => [key, count + 2])), 'queued refreshes coalesce into one complete trailing snapshot');
      assert.equal(maxSystemActive, 1, 'snapshot batches never overlap');

      await page.getByRole('link', { name: `选择实例 ${client.name}`, exact: true }).click();
      await page.getByRole('button', { name: '查看与控制', exact: true }).click();
      const dialog = page.getByRole('dialog', { name: client.name, exact: true });
      const movement = dialog.getByRole('button', { name: '云台向上', exact: true });
      await movement.focus(); await page.keyboard.down('Space');
      await expect.poll(() => typeof releaseMove).toBe('function');
      await page.keyboard.up('Space');
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      assert.deepEqual(commands.map(value => value.action), ['move'], 'stop waits for the outstanding move response');
      releaseMove();
      await expect.poll(() => commands.map(value => value.action)).toEqual(['move', 'stop']);
      const retry = dialog.getByRole('button', { name: '重试状态查询', exact: true });
      await expect(retry).toBeVisible();
      const beforeRetry = commands.length;
      await retry.click();
      await expect(dialog.getByRole('status').filter({ hasText: '设备已确认执行' })).toBeVisible();
      assert.equal(commands.length, beforeRetry, 'status recovery never repeats a physical POST');
      assert.equal(queries.length, 2);
      assert.ok(queries.every(query => query.method === 'GET' && query.path === queries[0].path));
      await page.keyboard.press('Escape');
      const settings = page.getByRole('region', { name: '实例设置', exact: true });
      const name = settings.getByLabel('实例名称', { exact: true });
      const save = settings.getByRole('button', { name: '保存名称', exact: true });
      await name.fill('Renamed'); await save.click();
      await expect.poll(() => typeof releaseRename).toBe('function');
      await expect(name).toHaveJSProperty('readOnly', true);
      await name.evaluate(input => input.form.requestSubmit());
      releaseRename();
      await expect(settings.getByRole('alert')).toBeVisible();
      await expect(name).toHaveValue('Renamed');
      await expect(name).toHaveJSProperty('readOnly', false);
      assert.deepEqual(names, ['Renamed'], 'a pending rename cannot send another PATCH');
      failClientReads = true;
      await save.click();
      await expect(settings).toContainText('实例名称已保存，但列表刷新失败');
      assert.deepEqual(names, ['Renamed', 'Renamed']);
      await name.fill('Newer draft');
      failClientReads = false;
      await settings.getByRole('button', { name: '重试刷新', exact: true }).click();
      await expect(page.getByRole('region', { name: '配对账户信息', exact: true })).toContainText('Renamed');
      await expect(name).toHaveValue('Newer draft');
      assert.equal(names.length, 2, 'retrying the list read cannot repeat a successful PATCH or discard a newer draft');
      failClientReads = true;
      await save.click();
      await expect(settings).toContainText('实例名称已保存，但列表刷新失败');
      await name.fill(client.name);
      await expect(save).toBeEnabled();
      await save.click();
      await expect(settings).toContainText('实例名称已保存，但列表刷新失败');
      assert.deepEqual(names, ['Renamed', 'Renamed', 'Newer draft', client.name], 'a user can revert a saved name while the list snapshot is stale');
      failClientReads = false;
      await settings.getByRole('button', { name: '重试刷新', exact: true }).click();
      await expect(save).toBeDisabled();
      currentClient = { ...currentClient, status: 'revoked' };
      await page.evaluate(() => window.concurrencySource.dispatchEvent(new Event('open')));
      await expect(name).toHaveJSProperty('readOnly', true);
      await name.evaluate(input => { input.value = 'Forbidden rename'; input.dispatchEvent(new Event('input', { bubbles: true })); input.form.requestSubmit(); });
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      assert.equal(names.length, 4, 'a revoked instance cannot be renamed through a submitted form');
      await page.evaluate(() => { window.concurrencyNotices = []; });
      for (const [index, kind] of ['__proto__', 'constructor', 'toString'].entries()) {
        await page.evaluate(kind => window.concurrencySource.dispatchEvent(new MessageEvent('system-event', { data: JSON.stringify({ kind }) })), kind);
        await expect.poll(() => page.evaluate(() => window.concurrencyNotices)).toEqual(Array(index + 1).fill('未识别的事件'));
        await expect(page.getByText('未识别的事件', { exact: true }).last()).toBeVisible();
      }
      assert.equal(wasm.length, 1);
      assert.deepEqual(errors, []);
      console.log(`${engine.name()}: actual WASM snapshot coalescing, serialized PTZ stop, read-only status recovery and unknown labels passed`);
      await page.close();
    } finally { await browser.close(); }
  }
} finally { await server.close(); }
