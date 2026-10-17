import "@xcss/web/design-tokens/tokens.css";
import "@xcss/web/design-tokens/tokens.dark.css";
import "@xcss/web/web-fonts/fonts.css";
import "@xcss/web/admin-ui/styles.css";
import "@xcss/web/admin-ui/date-range.css";
import "@xcss/web/design-tokens/reset.css";
import "@xcss/web/design-tokens/accessibility.css";
import "./styles.css";
import { startServerCorners } from "@xcss/web/admin-ui/server-corners";
import initialize from "../pkg/xcos_web.js";

// Browser UI, state, protocol handling and authentication run in Rust/xcss-web.
// This module only loads Vite-managed CSS/assets and the WASM module.
startServerCorners(document);
await initialize();
