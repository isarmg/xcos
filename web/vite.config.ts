import { xcssFontLicenses, xssiRuntimeInventory } from "./font-licenses.mjs";
import { createXcssReactViteConfig } from "@xcss/web/web-toolchain/vite";
import { mergeConfig } from "vite";

export default async () => mergeConfig(createXcssReactViteConfig(), {
  plugins: [xcssFontLicenses(), ...await xssiRuntimeInventory()],
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
