// Runs the default production entry with storage deliberately unavailable.
import assert from "node:assert/strict";
import { chromium, firefox, expect } from "@playwright/test";
import { preview } from '../../tools/web-build/test-preview.mjs';

const session = { authenticated: true, user_id: "A".repeat(43), username: "admin", role: "admin", csrf_token: "A".repeat(43) };
const server = await preview({ preview: { host: "127.0.0.1", port: 0, strictPort: true } });
try {
  for (const engine of [chromium, firefox]) {
    const browser = await engine.launch();
    try {
      const page = await browser.newPage({ locale: "zh-CN" });
      const errors = [];
      let authenticated = false;
      page.on("pageerror", error => errors.push(error.message));
      await page.emulateMedia({ colorScheme: "light" });
      await page.addInitScript(() => {
        for (const name of ["getItem", "setItem", "removeItem"]) {
          Object.defineProperty(Storage.prototype, name, { configurable: true, value() { throw new DOMException("Storage denied", "SecurityError"); } });
        }
      });
      await page.route("**/api/v1/**", async route => {
        const request = route.request(), path = new URL(request.url()).pathname;
        if (path === "/api/v1/auth/session") return authenticated
          ? route.fulfill({ json: session })
          : route.fulfill({ status: 401, contentType: "application/problem+json", body: JSON.stringify({ status: 401, code: "auth.required" }) });
        if (path === "/api/v1/auth/login") {
          assert.equal(request.method(), "POST");
          assert.deepEqual(request.postDataJSON(), { username: "admin", password: "fixture-password-strong" });
          authenticated = true;
          return route.fulfill({ json: session });
        }
        if (path === "/api/v1/clients" || path === "/api/v1/cameras") return route.fulfill({ json: [] });
        if (path === "/api/v1/system/status") return route.fulfill({ json: { service: "xcos", version: "1.0.1", database: "ok", media_service: "ok", cameras: { recording_configured: 0 }, server_time: "2026-10-16T00:00:00Z" } });
        if (path === "/api/v1/events/stream") return route.fulfill({ status: 204 });
        throw new Error(`Unexpected startup request ${path}`);
      });
      const wasm = page.waitForResponse(response => new URL(response.url()).pathname.endsWith(".wasm") && response.status() === 200);
      await page.goto(`${server.resolvedUrls.local[0]}?lang=en#instances`);
      await wasm;
      await expect(page.locator("html")).toHaveAttribute("data-xcss-runtime", "leptos", { timeout: 30_000 });
      await expect(page.locator("html")).toHaveAttribute("lang", "en");
      await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
      await expect(page.getByRole("heading", { name: "Administrator sign in", exact: true })).toBeVisible();
      await page.emulateMedia({ colorScheme: "dark" });
      await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
      await page.getByRole("button", { name: "Switch to light mode", exact: true }).click();
      await page.emulateMedia({ colorScheme: "light" });
      await page.emulateMedia({ colorScheme: "dark" });
      await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
      await page.getByRole("textbox", { name: "Username", exact: true }).fill("admin");
      await page.getByLabel("Password", { exact: true }).fill("fixture-password-strong");
      await page.getByRole("button", { name: "Sign in", exact: true }).click();
      await expect(page.getByRole("button", { name: "Instance list", exact: true })).toBeVisible();
      await expect(page.locator("html")).toHaveAttribute("lang", "en");
      await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
      assert.equal(new URL(page.url()).searchParams.get("lang"), "en");
      assert.deepEqual(errors, []);
      await page.close();
      console.log(`${engine.name()}: default WASM entry, system/manual theme and explicit language login with denied storage passed`);
    } finally { await browser.close(); }
  }
} finally { await server.close(); }
