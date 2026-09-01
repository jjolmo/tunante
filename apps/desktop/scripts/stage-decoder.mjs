// Build tunante-decoder and put it where the desktop app will find it.
//
// The app decodes nothing in-process since fase 1 of docs/plan-desktop-slint.md:
// every format arrives as PCM from the tunante-decoder helper, so the helper has
// to travel with the app. Two destinations, one script:
//
//   node stage-decoder.mjs --dev
//       Copies a RELEASE decoder next to the app's DEBUG binary
//       (target/debug/), where tunante-helper's sibling lookup finds it.
//       Release on purpose: the emulator cores are far too slow in debug to
//       play anything, the same reason the format smoke test runs --release.
//
//   node stage-decoder.mjs
//       Stages target/release/tunante-decoder as the Tauri sidecar
//       (src-tauri/binaries/tunante-decoder-<host triple>), which the bundler
//       renames back to a plain sibling inside the AppImage/.app/installer.
//
// Node rather than shell because the Windows release job has no sh in npm's
// default script shell.

import { execSync } from "node:child_process";
import { mkdirSync, copyFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const appDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = join(appDir, "..", "..");
const exe = process.platform === "win32" ? ".exe" : "";

execSync("cargo build --release -p tunante-decoder", { cwd: repoRoot, stdio: "inherit" });
const built = join(repoRoot, "target", "release", `tunante-decoder${exe}`);

if (process.argv.includes("--dev")) {
  const dest = join(repoRoot, "target", "debug");
  mkdirSync(dest, { recursive: true });
  copyFileSync(built, join(dest, `tunante-decoder${exe}`));
  console.log(`staged release tunante-decoder into ${dest}`);
} else {
  const host = /host: (\S+)/.exec(execSync("rustc -vV").toString())[1];
  const dest = join(appDir, "src-tauri", "binaries");
  mkdirSync(dest, { recursive: true });
  copyFileSync(built, join(dest, `tunante-decoder-${host}${exe}`));
  console.log(`staged tunante-decoder sidecar for ${host}`);
}
