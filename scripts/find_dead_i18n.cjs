const fs = require("fs");
const path = require("path");
function walk(d, acc) {
  for (const f of fs.readdirSync(d)) {
    const p = path.join(d, f);
    const s = fs.statSync(p);
    if (s.isDirectory()) walk(p, acc);
    else if (/\.(tsx?|jsx?)$/.test(f)) acc.push(p);
  }
  return acc;
}
const files = walk("src", []);
const src = files.map((f) => fs.readFileSync(f, "utf8")).join("\n");

// Collect template-literal t(`prefix${x}suffix`) patterns → regexes that keep keys alive
const templateRes = [];
const tRe = /\bt\(\s*`([^`]+)`/g;
let m;
while ((m = tRe.exec(src))) {
  const tpl = m[1];
  // Escape regex specials, then turn ${...} into a wildcard
  const esc = tpl.replace(/[.*+?^${}()|[\]\\]/g, (ch) =>
    ch === "$" ? "$" : ch === "{" ? "\\{" : ch === "}" ? "\\}" : "\\" + ch
  );
  // Simpler: split on ${...}
  const parts = tpl.split(/\$\{[^}]+\}/);
  const pattern =
    "^" +
    parts
      .map((p) =>
        p
          .replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
      )
      .join(".+") +
    "$";
  templateRes.push(new RegExp(pattern));
}

const en = JSON.parse(fs.readFileSync("src/i18n/en.json", "utf8"));
function flatten(o, p, out) {
  for (const k of Object.keys(o)) {
    const v = o[k];
    const key = p ? p + "." + k : k;
    if (v && typeof v === "object" && !Array.isArray(v)) flatten(v, key, out);
    else out.push(key);
  }
  return out;
}
const keys = flatten(en, "", []);
const dead = keys.filter(
  (k) =>
    k &&
    !src.includes('"' + k + '"') &&
    !templateRes.some((re) => re.test(k))
);
console.log(JSON.stringify(dead, null, 2));
console.log("TOTAL_KEYS", keys.length, "DEAD", dead.length, "TEMPLATES", templateRes.length);
