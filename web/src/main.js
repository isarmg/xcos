import "../xcss-web/vendor/xcss-web/startup.css";
import { startApplication } from "../xcss-web/vendor/xcss-web/startup.js";
import "@xcss/web/design-tokens/tokens.css";
import "@xcss/web/design-tokens/tokens.dark.css";
import "@xcss/web/web-fonts/fonts.css";
import "@xcss/web/admin-ui/styles.css";
import "@xcss/web/admin-ui/date-range.css";
import "@xcss/web/design-tokens/reset.css";
import "@xcss/web/design-tokens/accessibility.css";
import "./styles.css";
import { startServerCorners } from "@xcss/web/admin-ui/server-corners";

// Browser UI, state, protocol handling and authentication run in Rust/xcss-web.
// This module only loads authored styles/assets and the WASM module.
await startApplication(async () => {
  startServerCorners(document);
  const { default: initialize } = await import("../pkg/xcos_web.js");
  await initialize();
});
