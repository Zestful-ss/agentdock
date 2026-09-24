#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';

const root = process.cwd();
const read = (file) => fs.readFileSync(path.join(root, file), 'utf8');
const json = (file) => JSON.parse(read(file));

const tauri = json('src-tauri/tauri.conf.json');
const cargo = read('src-tauri/Cargo.toml');
const workflow = read('.github/workflows/release.yml');
const hook = read('src-tauri/windows/agentdock-migration.nsh');

const checks = [
  ['Tauri product name', tauri.productName === 'AgentDock'],
  ['legacy main binary name', tauri.mainBinaryName === 'skills-manager'],
  ['legacy macOS bundle name', tauri.bundle?.macOS?.bundleName === 'skills-manager'],
  ['pinned MSI upgrade code', tauri.bundle?.windows?.wix?.upgradeCode === '08c191c4-bdbb-5964-9f50-bc2b2edafd1a'],
  ['NSIS migration hook configured', tauri.bundle?.windows?.nsis?.installerHooks === 'windows/agentdock-migration.nsh'],
  ['legacy NSIS install path migration', hook.includes('Software\\agentskills\\skills-manager')],
  ['Debian package replacement', tauri.bundle?.linux?.deb?.replaces?.includes('skills-manager')],
  ['RPM package obsoletion', tauri.bundle?.linux?.rpm?.obsoletes?.includes('skills-manager')],
  ['Cargo package rename', /^name = "agentdock"/m.test(cargo)],
  ['new CLI target', /^name = "agentdock-cli"/m.test(cargo)],
  ['legacy CLI target', /^name = "skills-manager-cli"/m.test(cargo)],
  ['release source guard', workflow.includes('RELEASE_SOURCE_REPOSITORY')],
  [
    'updater repository alignment',
    tauri.plugins?.updater?.endpoints?.[0]?.includes('/xingkongliang/skills-manager/'),
  ],
  ['Linux ARM64 updater check', workflow.includes('linux-aarch64')],
];

const failed = checks.filter(([, ok]) => !ok).map(([name]) => name);
if (failed.length) {
  console.error(`AgentDock migration checks failed: ${failed.join(', ')}`);
  process.exit(1);
}

console.log(`AgentDock migration checks passed (${checks.length} checks)`);
