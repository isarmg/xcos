import {readFileSync} from "node:fs";
import {fileURLToPath, pathToFileURL} from "node:url";
import {join} from "node:path";
export async function xssiRuntimeInventory() {
  if (!process.env.XSSI_BUILD_TOOLS_DIR) return [];
  if (!process.env.XSSI_FRONTEND_METADATA_FILE) throw new Error("Explicit runtime inventory output is required");
  const {runtimeInventory} = await import(pathToFileURL(join(process.env.XSSI_BUILD_TOOLS_DIR,"scripts/frontend_build.mjs")));
  return [runtimeInventory({root:fileURLToPath(new URL("./",import.meta.url)),output:process.env.XSSI_FRONTEND_METADATA_FILE})];
}
export function xcssFontLicenses(prefix = "assets/") {
  const directory = new URL("./", import.meta.resolve("@xcss/web/web-fonts/OFL.txt"));
  return {
    name: "xcss-font-licenses",
    generateBundle() {
      for (const name of ["OFL.txt", "CJK-LICENSE.txt", "NORMAL-LICENSE.txt"]) {
        this.emitFile({type: "asset", fileName: prefix + name, source: readFileSync(new URL(name, directory))});
      }
    },
  };
}
