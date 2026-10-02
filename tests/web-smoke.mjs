import { readFileSync } from "node:fs";
import init, { run_wasm } from "../pkg/refinement_microegg.js";

await init({
  module_or_path: readFileSync(new URL("../pkg/refinement_microegg_bg.wasm", import.meta.url)),
});

for (const [name, expected] of [
  ["refinement.sexp", "refinement guards passed"],
  ["dontcare.sexp", "x"],
  ["set_algebra.sexp", "A"],
  ["ac_le.sexp", "AC and order passed"],
]) {
  const program = readFileSync(new URL(`../examples/${name}`, import.meta.url), "utf8");
  const output = run_wasm(program);
  if (output !== expected) {
    throw new Error(`${name}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(output)}`);
  }
  console.log(`ok ${name}`);
}
