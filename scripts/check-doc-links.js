#!/usr/bin/env node
/**
 * Check relative links in Markdown docs (issue #1044).
 *
 * For every `[text](target)` that is not an external URL, verifies that the
 * target file exists and, when it has a `#fragment`, that the file contains a
 * heading whose GitHub-style slug matches. External links are skipped.
 *
 * Usage: node scripts/check-doc-links.js [file.md ...]   (default: docs/faq.md)
 * Exits 1 and lists every broken link if any are found.
 */
const fs = require("fs");
const path = require("path");

// GitHub heading slug: lowercase, drop punctuation except - and _, spaces → -.
const slug = (heading) =>
  heading
    .trim()
    .toLowerCase()
    .replace(/[`*]/g, "")
    .replace(/[^\p{L}\p{N} _-]/gu, "")
    .replace(/ /g, "-");

const anchorsOf = (file) =>
  new Set(
    fs
      .readFileSync(file, "utf8")
      .split("\n")
      .filter((line) => /^#{1,6} /.test(line))
      .map((line) => slug(line.replace(/^#+ /, ""))),
  );

const files = process.argv.slice(2).length ? process.argv.slice(2) : ["docs/faq.md"];
const broken = [];

for (const file of files) {
  const text = fs.readFileSync(file, "utf8").replace(/```[\s\S]*?```/g, "");
  for (const [, target] of text.matchAll(/\]\(([^)\s]+)\)/g)) {
    if (/^[a-z]+:/i.test(target)) continue;
    const [rel, fragment] = target.split("#");
    const resolved = rel ? path.resolve(path.dirname(file), rel) : path.resolve(file);
    if (!fs.existsSync(resolved)) {
      broken.push(`${file}: ${target} (missing file)`);
    } else if (fragment && resolved.endsWith(".md") && !anchorsOf(resolved).has(fragment)) {
      broken.push(`${file}: ${target} (missing anchor)`);
    }
  }
}

if (broken.length) {
  console.error(`Broken links:\n  ${broken.join("\n  ")}`);
  process.exit(1);
}
console.log(`All relative links OK in ${files.join(", ")}`);
