#!/usr/bin/env node
// Fails when site/src/data/release.json does not name the release being built.
//
// The site regenerates release.json from the published GitHub release at
// deploy, so the committed copy is only ever read by a local build — and a
// committed copy naming an older version is how the download page came to
// serve links to a release that was two versions behind. The version bump
// moves this file, and this is what makes a tag build stop if it did not.
//
// "The current release" is the version in src-tauri/tauri.conf.json, which is
// the same number the workspace and both package.json files carry. Pass
// --tag vX.Y.Z to require the tag being built to agree with it too.
//
// Usage: node scripts/check-release-json.mjs [--tag vX.Y.Z]

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function read(path) {
  return JSON.parse(readFileSync(join(root, path), "utf8"));
}

function main(argv) {
  const tagIndex = argv.indexOf("--tag");
  const tag = tagIndex === -1 ? null : argv[tagIndex + 1];

  const version = read("src-tauri/tauri.conf.json").version;
  const release = read("site/src/data/release.json");
  const problems = [];

  if (tag && tag !== `v${version}`) {
    problems.push(`the tag ${tag} does not match version ${version} in src-tauri/tauri.conf.json`);
  }
  if (release.version !== version) {
    problems.push(
      `site/src/data/release.json names version ${release.version}, the release is ${version}`,
    );
  }
  if (release.tag !== `v${version}`) {
    problems.push(
      `site/src/data/release.json names tag ${release.tag}, the release is v${version}`,
    );
  }

  if (problems.length > 0) {
    for (const problem of problems) console.error(`release.json: ${problem}`);
    console.error("Run .github/scripts/bump_version.py to move every version file at once.");
    return 1;
  }
  console.log(`release.json names ${release.tag}, the release being built.`);
  return 0;
}

process.exit(main(process.argv.slice(2)));
