import {prepareApplicationFonts, startAfterFonts} from "@xcss/web/web-fonts";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";


import manifest from "../package.json" with { type: "json" };

const lock = JSON.parse(
  readFileSync(new URL("../package-lock.json", import.meta.url), "utf8"),
);
// One exact xcss input version, with tarball integrity and no local links.

const xcssPackages = ["@xcss/web"];

const nodeVersion = readFileSync(
  new URL("../../.node-version", import.meta.url),
  "utf8",
);
assert.match(nodeVersion, /^26\.7\.0\n?$/);
assert.equal(manifest.dependencies.react, undefined);
assert.equal(manifest.dependencies["react-dom"], undefined);
assert.equal(manifest.devDependencies["@vitejs/plugin-react"], undefined);
assert.equal(manifest.devDependencies.vite, "8.3.3");
assert.match(manifest.scripts.build, /build-leptos\.py build/);
assert.match(manifest.scripts.typecheck, /build-leptos\.py check/);
for (const dependency of xcssPackages) {
  const expected = "https://github.com/isarmg/xcss/releases/download/v1.0.0/xcss-web-1.0.0.tgz";
  assert.equal(manifest.dependencies?.[dependency], expected);
  assert.equal(lock.packages?.[""]?.dependencies?.[dependency], expected);

  const locked = lock.packages?.[`node_modules/${dependency}`];
  assert.equal(locked?.link, undefined);
  assert.equal(locked?.resolved, expected);
  assert.equal(locked?.version, "1.0.0");
  assert.equal(locked?.integrity, "sha512-QR/zmdzyQkLnFzTTp+EHYnOL9XEe5N95XVUasnDlKqdNWw/nVoY4evq083QTxMED/ElRRYeegEItnTxCUcb7TQ==");
}
const adminStyles = readFileSync(new URL("../node_modules/@xcss/web/dist/admin-ui/styles.css", import.meta.url), "utf8");
assert.match(adminStyles, /@import ["']\.\/content-blocks\.css["']/);
for (const snapshot of ["content-blocks.css", "provenance.json", "verify.mjs"]) {
  assert.equal(existsSync(new URL(`../appearance/${snapshot}`, import.meta.url)), false, "xcss CSS must not be copied into the product");
}

const main = readFileSync(new URL("../leptos/src/lib.rs", import.meta.url), "utf8");
const bootstrap = readFileSync(new URL("../src/main.js", import.meta.url), "utf8");
const browserManifest = readFileSync(new URL("../leptos/Cargo.toml", import.meta.url), "utf8");
assert.match(browserManifest, /leptos = .*csr/);
assert.match(bootstrap, /pkg\/xcos_web\.js/);
assert.doesNotMatch(bootstrap, /react|React|createRoot/);
assert.match(readFileSync(new URL("../index.html", import.meta.url), "utf8"), /src="\/src\/main\.js"/);
const styles = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
assert.match(main, /AdminShell product="xcos"/);
assert.doesNotMatch(main, /AdministratorsPanel|管理员账号|Administrator account/);
assert.doesNotMatch(main, /useAdministratorSession|LoginScreen|UserEditor|ManagedUser|window\.confirm|["']\/users/);
assert.doesNotMatch(styles, /--xcss-[a-z-]+\s*:|font-family|login-screen|side-rail|toast-stack/);

assert.equal(typeof prepareApplicationFonts, "function");
assert.equal(typeof startAfterFonts, "function");
