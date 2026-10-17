import { xcssFontLicenses, xssiRuntimeInventory } from "./font-licenses.mjs";
import { defineConfig } from "vite";
import { xcssWebStaticAssets } from "./xcss-web-assets.mjs";
import { readFileSync } from "node:fs";

export default async () => defineConfig({
  plugins: [xcssFontLicenses(), ...await xssiRuntimeInventory(), xcssWebStaticAssets(), {
    name: "xcos-xcss-web-asset-budget",
    generateBundle(_options, bundle) {
      this.emitFile({ type: "asset", fileName: "assets/xcss-web-build.json", source: readFileSync(new URL("./pkg/xcss-web-build.json", import.meta.url)) });
      let total = 0;
      for (const [name, asset] of Object.entries(bundle)) {
        const size = asset.type === "chunk" ? Buffer.byteLength(asset.code) : typeof asset.source === "string" ? Buffer.byteLength(asset.source) : asset.source.byteLength;
        total += size;
        // WASM contains the complete Rust runtime and compiled application.
        const limit = /\.woff2?$/i.test(name) ? 16 * 1024 * 1024 : /\.wasm(?:\.gz)?$/u.test(name) ? 4 * 1024 * 1024 : 512 * 1024;
        if (name.endsWith(".map")) this.error("Frontend builds must not publish source maps");
        if (size > limit) this.error(`Asset ${name} is ${size} bytes; budget is ${limit}`);
      }
      if (total > 16 * 1024 * 1024) this.error("Frontend exceeds the 16 MiB total asset budget");
    },
  }],
  build: { target: "esnext", cssTarget: "esnext", modulePreload: { polyfill: false }, outDir: process.env.XCSS_WEB_DIST ?? "dist", emptyOutDir: true, sourcemap: false, assetsInlineLimit: 0, chunkSizeWarningLimit: 512 },
  server: {
    port: 5173,
    proxy: {
      "/api/v1": "http://127.0.0.1:18106",
      "/healthz": "http://127.0.0.1:18106",
      "/readyz": "http://127.0.0.1:18106",
      "/media-webrtc": {
        target: "http://127.0.0.1:8889",
        rewrite: (path: string) => path.replace(/^\/media-webrtc/, ""),
      },
      "/media-hls": {
        target: "http://127.0.0.1:8888",
        rewrite: (path: string) => path.replace(/^\/media-hls/, ""),
      },
    },
  },
});
