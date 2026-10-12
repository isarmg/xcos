import assert from "node:assert/strict";
import { chromium, firefox, expect } from "@playwright/test";
import { preview } from "vite";

const id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const session = { authenticated: true, user_id: "A".repeat(43), username: "admin", role: "admin", csrf_token: "A".repeat(43) };
const client = { id, installation_id: id, name: "Clock fixture", client_version: "1.1.0", authorization_code: "a".repeat(36), status: "online" };
const capabilities = { video: "supported", main_stream: "supported", sub_stream: "unsupported", local_recording: "unsupported", server_recording: "supported", ptz: "unsupported", events: "unsupported", audio_input: "unsupported", audio_output: "unsupported" };
const server = await preview({ preview: { host: "127.0.0.1", port: 0, strictPort: true } });
const address = server.httpServer.address();
assert.ok(address && typeof address === "object");

try {
  for (const engine of [chromium, firefox]) {
    const browser = await engine.launch();
    try {
      for (const initialOffsetMs of [0, 120_000, -120_000]) {
        const context = await browser.newContext({ locale: "zh-CN" });
        try {
          // Keep the monotonic browser clock intact while simulating wrong wall time
          // and later operating-system clock adjustments in the same document.
          await context.addInitScript(offset => {
            const realNow = Date.now.bind(Date);
            window.__xcosClockOffsetMs = offset;
            Date.now = () => realNow() + window.__xcosClockOffsetMs;
          }, initialOffsetMs);
          const page = await context.newPage();
          const errors = [];
          page.on("pageerror", error => errors.push(error.message));
          await page.route("**/api/v1/**", route => {
            const path = new URL(route.request().url()).pathname;
            const now = Date.now();
            const at = new Date(now).toISOString();
            if (path.endsWith("/auth/session")) return route.fulfill({ json: session });
            if (path.endsWith("/clients")) return route.fulfill({ json: [{ ...client, last_seen_at: at, created_at: at, updated_at: at }] });
            if (path.endsWith("/cameras")) return route.fulfill({ json: [{
              id, name: "Clock fixture", location: "", has_sub_stream: false, source_kind: "client", client_id: id,
              adapter_kind: "rtsp", manufacturer: null, model: null, firmware_version: null, serial_number: null,
              capabilities, streams: [{ profile: "main", video_codec: "h264", audio_codec: null, width: 1280, height: 720, frame_rate: 30 }],
              health_message: null, device_status: "online", storage_mode: "server", enabled: true, record_enabled: true, status: "online",
              observation_status: "fresh", last_observed_at: at, observation_expires_at: new Date(now + 60_000).toISOString(),
              observation_remaining_ms: 60_000, last_seen_at: at, created_at: at, updated_at: at,
            }] });
            // Stop event reconnects: the regression exercises the existing camera
            // snapshot across wall-clock changes rather than replacing it via SSE.
            if (path.endsWith("/events/stream")) return route.fulfill({ status: 204 });
            if (path.endsWith("/system/status")) return route.fulfill({ json: { service: "xcos", version: "1.0.3", database: "ok", media_service: "ok", cameras: { recording_configured: 1 }, server_time: at } });
            if (path.endsWith("/stream-ticket")) return route.fulfill({ status: 503, json: { code: "media.unavailable", message: "unavailable" } });
            throw new Error(`Unexpected API request ${route.request().method()} ${path}`);
          });
          await page.goto(`http://127.0.0.1:${address.port}/#instances`);
          const table = page.getByRole("table", { name: "摄像机实例列表", exact: true });
          const cameraStatus = table.locator("tbody tr").locator("td").nth(1);
          await expect(cameraStatus).toHaveText("在线");
          // Each row starts with a <th>, so td[1] is the camera-status column,
          // after td[0] containing the independent client pairing status.
          await page.evaluate(async () => { await document.fonts.ready; });
          for (const changedOffsetMs of [6 * 60 * 60_000, -6 * 60 * 60_000]) {
            await page.evaluate(offset => { window.__xcosClockOffsetMs = offset; }, changedOffsetMs);
            await table.getByRole("link", { name: "选择实例 Clock fixture", exact: true }).click();
            await expect(page.getByRole("region", { name: "摄像机状态", exact: true }).locator(".camera-status")).toHaveText("在线");
            await expect(page.locator(".camera-card .camera-status")).toHaveText("在线");
            await page.getByRole("button", { name: "实例列表", exact: true }).click();
            await expect(cameraStatus).toHaveText("在线");
          }
          assert.deepEqual(errors, []);
          console.log(`${engine.name()}: initial wall-clock offset ${initialOffsetMs} ms and subsequent ±6-hour jumps preserve camera online status`);
        } finally {
          await context.close();
        }
      }
    } finally {
      await browser.close();
    }
  }
} finally {
  await new Promise(resolve => server.httpServer.close(resolve));
}
