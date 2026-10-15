// Checks every file in /regions and says, in plain words, what is wrong with
// any that Coin Heaven could not use. Run it with `pnpm check:regions` after adding
// or editing a country; it needs nothing but Node.
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseRegion } from "../src/lib/regionSchema.ts";

const dir = join(dirname(fileURLToPath(import.meta.url)), "..", "regions");
const files = readdirSync(dir).filter((f) => f.endsWith(".json") && !f.endsWith(".schema.json"));

let bad = 0;
for (const file of files.sort()) {
  let raw;
  try {
    raw = JSON.parse(readFileSync(join(dir, file), "utf8"));
  } catch (e) {
    bad++;
    console.log(`✗ ${file}\n    is not valid JSON: ${e.message}`);
    continue;
  }
  const { region, problems } = parseRegion(raw, file.replace(/\.json$/, ""));
  if (region) {
    const bands = region.fire.taxBrackets.length;
    console.log(
      `✓ ${file}  ${region.name} · ${region.currency} · ${bands === 0 ? "no tax bands" : `${bands} tax band${bands === 1 ? "" : "s"}`}`,
    );
  } else {
    bad++;
    console.log(`✗ ${file}`);
    for (const p of problems) console.log(`    ${p}`);
  }
}
console.log(bad === 0 ? `\n${files.length} region file(s), all good.` : `\n${bad} file(s) need fixing.`);
process.exit(bad === 0 ? 0 : 1);
