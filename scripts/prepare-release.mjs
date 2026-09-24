#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const root = process.cwd();
const args = process.argv.slice(2);

const releaseArg = args.find((arg) => !arg.startsWith('--'));
const dryRun = args.includes('--dry-run');
const dateStr = new Date().toISOString().slice(0, 10);

if (!releaseArg) {
  console.error('Usage: npm run release:prepare -- <patch|minor|major|x.y.z> [--dry-run]');
  process.exit(1);
}

const packagePath = path.join(root, 'package.json');
const packageLockPath = path.join(root, 'package-lock.json');
const tauriConfPath = path.join(root, 'src-tauri', 'tauri.conf.json');
const cargoTomlPath = path.join(root, 'src-tauri', 'Cargo.toml');
const cargoLockPath = path.join(root, 'src-tauri', 'Cargo.lock');
const enI18nPath = path.join(root, 'src', 'i18n', 'en.json');
const zhI18nPath = path.join(root, 'src', 'i18n', 'zh.json');
const changelogPath = path.join(root, 'CHANGELOG.md');
const changelogZhPath = path.join(root, 'CHANGELOG-zh.md');

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

function writeJson(filePath, value) {
  fs.writeFileSync(filePath, `${JSON.stringify(value, null, 2)}\n`);
}

function parseSemver(version) {
  const m = version.match(/^(\d+)\.(\d+)\.(\d+)$/);
  if (!m) return null;
  return { major: Number(m[1]), minor: Number(m[2]), patch: Number(m[3]) };
}

function bumpVersion(current, releaseType) {
  const parsed = parseSemver(current);
  if (!parsed) {
    throw new Error(`Current package version is not SemVer: ${current}`);
  }

  if (releaseType === 'patch') {
    return `${parsed.major}.${parsed.minor}.${parsed.patch + 1}`;
  }
  if (releaseType === 'minor') {
    return `${parsed.major}.${parsed.minor + 1}.0`;
  }
  if (releaseType === 'major') {
    return `${parsed.major + 1}.0.0`;
  }

  if (parseSemver(releaseType)) {
    return releaseType;
  }

  throw new Error(`Invalid release type/version: ${releaseType}`);
}

function updateSettingsVersion(i18nObj, nextVersion, fileLabel) {
  if (!i18nObj.settings || typeof i18nObj.settings.version !== 'string') {
    throw new Error(`Missing settings.version in ${fileLabel}`);
  }
  i18nObj.settings.version = i18nObj.settings.version.replace(/\d+\.\d+\.\d+/, nextVersion);
}

function updateCargoPackageVersion(cargoToml, nextVersion) {
  const packageStart = cargoToml.indexOf('[package]');
  if (packageStart === -1) {
    throw new Error('Missing [package] in src-tauri/Cargo.toml');
  }
  const nextSection = cargoToml.indexOf('\n[', packageStart + '[package]'.length);
  const packageEnd = nextSection === -1 ? cargoToml.length : nextSection;
  const packageSection = cargoToml.slice(packageStart, packageEnd);
  if (!/^version = "[^"]+"$/m.test(packageSection)) {
    throw new Error('Missing package version in src-tauri/Cargo.toml');
  }
  const updatedSection = packageSection.replace(
    /^version = "[^"]+"$/m,
    `version = "${nextVersion}"`,
  );
  return `${cargoToml.slice(0, packageStart)}${updatedSection}${cargoToml.slice(packageEnd)}`;
}

function updateCargoLockVersion(cargoLock, nextVersion) {
  const packagePattern = /(\[\[package\]\]\r?\nname = "agentdock"\r?\nversion = ")[^"]+("\r?\n)/;
  if (!packagePattern.test(cargoLock)) {
    throw new Error('Missing agentdock package entry in src-tauri/Cargo.lock');
  }
  return cargoLock.replace(
    packagePattern,
    (_match, prefix, suffix) => `${prefix}${nextVersion}${suffix}`,
  );
}

function sectionAfterHeading(text, headingPattern) {
  const match = headingPattern.exec(text);
  if (!match) return '';

  const afterHeading = text.slice(match.index + match[0].length);
  const nextHeading = afterHeading.search(/^## \[/m);
  return nextHeading === -1 ? afterHeading : afterHeading.slice(0, nextHeading);
}

function hasSubstantiveContent(section) {
  return section
    .split('\n')
    .map((line) => line.trim())
    .some((line) => {
      if (!line || line.startsWith('#')) return false;
      const isPlaceholder = line.startsWith('_') && line.endsWith('_');
      return !isPlaceholder;
    });
}

function updateChangelogDate(changelog, version, date) {
  const escapedVersion = version.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const headingPattern = new RegExp(`^(## \\[${escapedVersion}\\](?: - )?)(?:\\d{4}-\\d{2}-\\d{2})?$`, 'm');
  if (!headingPattern.test(changelog)) {
    throw new Error(`Missing ${version} heading while updating release date`);
  }
  return changelog.replace(
    headingPattern,
    `## [${version}] - ${date}`,
  );
}

function requireChangelogEntry(changelog, version, { zh = false, label = 'CHANGELOG.md' } = {}) {
  const escapedVersion = version.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const headingPattern = new RegExp(`^## \\[${escapedVersion}\\](?: - .*)?$`, 'm');
  const match = headingPattern.exec(changelog);
  if (!match) {
    throw new Error(`Missing ${label} release notes for ${version}; add a complete section before preparing the release`);
  }

  const afterHeading = changelog.slice(match.index + match[0].length);
  const nextHeading = afterHeading.search(/^## \[/m);
  const section = nextHeading === -1 ? afterHeading : afterHeading.slice(0, nextHeading);
  const headings = zh
    ? ['### 发布概览', '### 用户可见更新', '### 开发者与治理更新']
    : ['### Release Overview', '### User-facing', '### Developer & Governance'];
  for (const heading of headings) {
    if (!section.includes(heading)) {
      throw new Error(`${label} section ${version} is missing ${heading}`);
    }
  }

  const hasContent = section
    .split('\n')
    .some((line) => line.trim() && !line.startsWith('#') && line.trim() !== '-');
  if (!hasContent) {
    throw new Error(`${label} section ${version} contains only placeholders; add release notes before preparing the release`);
  }

  const unreleasedHeading = zh
    ? /^## \[未发布\][^\S\r\n]*$/m
    : /^## \[Unreleased\][^\S\r\n]*$/m;
  const unreleased = sectionAfterHeading(changelog, unreleasedHeading);
  if (hasSubstantiveContent(unreleased)) {
    throw new Error(`${label} still has substantive Unreleased content; fold it into ${version} before preparing a release`);
  }

  return changelog;
}

// Refresh the README star-history snapshot. Best-effort: a failure here (no gh
// auth, no network, no python3) must never block the version bump / changelog.
function refreshStarHistory() {
  const script = path.join(root, 'scripts', 'gen-star-history.py');
  const res = spawnSync('python3', [script], { stdio: 'inherit' });
  return !res.error && res.status === 0;
}

function main() {
  const pkg = readJson(packagePath);
  const tauriConf = readJson(tauriConfPath);
  const cargoToml = fs.readFileSync(cargoTomlPath, 'utf8');
  const cargoLock = fs.readFileSync(cargoLockPath, 'utf8');
  const en = readJson(enI18nPath);
  const zh = readJson(zhI18nPath);
  const changelog = fs.readFileSync(changelogPath, 'utf8');
  const changelogZh = fs.readFileSync(changelogZhPath, 'utf8');

  const currentVersion = pkg.version;
  const nextVersion = bumpVersion(currentVersion, releaseArg);

  pkg.version = nextVersion;
  // npm rewrites both of these on any install, so leaving them behind means
  // every contributor's `npm install` produces a stray diff. They sat at
  // 1.22.1 while package.json was 1.38.0 until #432 noticed.
  const packageLock = readJson(packageLockPath);
  packageLock.version = nextVersion;
  if (packageLock.packages?.['']) {
    packageLock.packages[''].version = nextVersion;
  }
  tauriConf.version = nextVersion;
  const nextCargoToml = updateCargoPackageVersion(cargoToml, nextVersion);
  const nextCargoLock = updateCargoLockVersion(cargoLock, nextVersion);
  updateSettingsVersion(en, nextVersion, 'src/i18n/en.json');
  updateSettingsVersion(zh, nextVersion, 'src/i18n/zh.json');
  const nextChangelog = updateChangelogDate(
    requireChangelogEntry(changelog, nextVersion, { label: 'CHANGELOG.md' }),
    nextVersion,
    dateStr,
  );
  const nextChangelogZh = updateChangelogDate(
    requireChangelogEntry(changelogZh, nextVersion, {
      zh: true,
      label: 'CHANGELOG-zh.md',
    }),
    nextVersion,
    dateStr,
  );

  if (dryRun) {
    console.log(`[dry-run] ${currentVersion} -> ${nextVersion}`);
    return;
  }

  writeJson(packagePath, pkg);
  writeJson(packageLockPath, packageLock);
  writeJson(tauriConfPath, tauriConf);
  fs.writeFileSync(cargoTomlPath, nextCargoToml);
  fs.writeFileSync(cargoLockPath, nextCargoLock);
  writeJson(enI18nPath, en);
  writeJson(zhI18nPath, zh);
  fs.writeFileSync(changelogPath, nextChangelog);
  fs.writeFileSync(changelogZhPath, nextChangelogZh);

  const starOk = refreshStarHistory();

  console.log(`Prepared release ${nextVersion}`);
  console.log('Updated:');
  console.log('- CHANGELOG.md');
  console.log('- CHANGELOG-zh.md');
  console.log('- package.json');
  console.log('- package-lock.json');
  console.log('- src-tauri/tauri.conf.json');
  console.log('- src-tauri/Cargo.toml');
  console.log('- src-tauri/Cargo.lock');
  console.log('- src/i18n/en.json');
  console.log('- src/i18n/zh.json');
  console.log(
    starOk
      ? '- assets/star-history.svg'
      : '- assets/star-history.svg (skipped: refresh failed — run `python3 scripts/gen-star-history.py` manually)',
  );
}

main();
