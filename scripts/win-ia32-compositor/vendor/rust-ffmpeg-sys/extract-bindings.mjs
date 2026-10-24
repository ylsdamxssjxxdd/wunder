import { execSync } from "node:child_process";
import { readdirSync, mkdirSync } from "node:fs";

const mapping = {
  "aarch64-unknown-linux-gnu.gz": "linux-arm",
  "aarch64-unknown-linux-musl.gz": "linux-arm-musl",
  "x86_64-unknown-linux-gnu.gz": "linux-x64",
  "x86_64-unknown-linux-musl.gz": "linux-x64-musl",
  "x86_64-apple-darwin.gz": "macos-x64",
  "aarch64-apple-darwin.gz": "macos-arm",
  "x86_64-pc-windows-gnu.gz": "windows",
};

for (const file of readdirSync("zips")) {
  if (file.includes("SOURCE")) {
    continue;
  }
  if (file.includes(".DS_Store")) {
    continue;
  }
  mkdirSync("bindings", { recursive: true });
  execSync(`tar xf zips/${file} --include='bindings.rs' -C bindings`);
  execSync(`cp bindings/bindings.rs bindings/${mapping[file]}.rs`);
  execSync(`mv bindings/bindings.rs bindings/${file.replace(".gz", "")}.rs`);
}
