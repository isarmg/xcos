import { startApplication } from "../xcss-web/vendor/xcss-web/startup.js";
import { createHash } from "node:crypto";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";


import manifest from "../package.json" with { type: "json" };
assert.deepEqual(manifest.dependencies ?? {}, {});

const lock = JSON.parse(
  readFileSync(new URL("../package-lock.json", import.meta.url), "utf8"),
);
assert.equal(manifest.devDependencies["@xcss/web"], undefined);
assert.equal(lock.packages?.["node_modules/@xcss/web"], undefined);

const nodeVersion = readFileSync(
  new URL("../../.node-version", import.meta.url),
  "utf8",
);
assert.match(nodeVersion, /^26\.7\.0\n?$/);
assert.equal(manifest.devDependencies.react, undefined);
assert.equal(manifest.devDependencies["react-dom"], undefined);
assert.equal(manifest.devDependencies["@vitejs/plugin-react"], undefined);
for (const dependency of ["vite", "typescript", "@types/node"]) {
  assert.equal(manifest.devDependencies[dependency], undefined);
}
assert.equal(manifest.scripts["test:unit"], "npm run test:rust");
assert.match(manifest.scripts["test:browser"], /xcss-web-concurrency\.mjs/);
assert.match(manifest.scripts.build, /web-build\/Cargo\.toml -- build/);
assert.match(manifest.scripts.typecheck, /web-build\/Cargo\.toml -- check/);
for (const vendor of ["xcss", "hls"]) {
  const root = new URL(`../vendor/${vendor}/`, import.meta.url);
  const source = JSON.parse(readFileSync(new URL("SOURCE.json", root), "utf8"));
  assert.equal(source.format, 1);
  for (const [file, digest] of Object.entries(source.files)) {
    assert.equal(createHash("sha256").update(readFileSync(new URL(file, root))).digest("hex"), digest, `${vendor}/${file}`);
  }
}
const adminStyles = readFileSync(new URL("../vendor/xcss/admin-ui/styles.css", import.meta.url), "utf8");
assert.match(adminStyles, /@import ["']\.\/content-blocks\.css["']/);
for (const snapshot of ["content-blocks.css", "provenance.json", "verify.mjs"]) {
  assert.equal(existsSync(new URL(`../appearance/${snapshot}`, import.meta.url)), false, "xcss CSS must not be copied into the product");
}

const main = readFileSync(new URL("../xcss-web/src/lib.rs", import.meta.url), "utf8");
const bootstrap = readFileSync(new URL("../src/main.js", import.meta.url), "utf8");
const browserManifest = readFileSync(new URL("../xcss-web/Cargo.toml", import.meta.url), "utf8");
assert.match(browserManifest, /xcss-web = .*csr/);
assert.match(bootstrap, /pkg\/xcos_web\.js/);
assert.doesNotMatch(bootstrap, /react|React|createRoot/);
assert.match(readFileSync(new URL("../index.html", import.meta.url), "utf8"), /src="\/src\/main\.js"/);
const styles = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
// Ignore generated lint attributes and formatting while retaining the shared
// shell renderer, its matching props builder, and the exact product binding.
const shellStructure = main.replace(/#\[[^\]]*\]/g, "").replace(/\s+/g, "");
assert.match(shellStructure, /component_view\(&AdminShell,\{letmutprops=component_props_builder\(&AdminShell\)\.product\(\{"xcos"\},?\)/);
assert.doesNotMatch(main, /AdministratorsPanel|管理员账号|Administrator account/);
assert.doesNotMatch(main, /useAdministratorSession|LoginScreen|UserEditor|ManagedUser|window\.confirm|["']\/users/);
assert.doesNotMatch(styles, /--xcss-[a-z-]+\s*:|font-family|login-screen|side-rail|toast-stack/);

assert.equal(typeof startApplication, "function");
