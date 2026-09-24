<p align="center">
  <img src="assets/icon.png" width="80" />
</p>

<h1 align="center">AgentDock</h1>

<p align="center">
  Manage Agent skills and MCP; observe Harnesses.
</p>

<p align="center">
  <strong><a href="https://skillsmanager.dev">skillsmanager.dev</a></strong>
</p>

<p align="center">
  <small>Compatibility: existing metadata stays under <code>~/.skills-manager</code>; the historical <code>skills-manager-cli</code> executable remains available as an alias during the transition. The GitHub release source is now <code>Zestful-ss/agentdock</code>; the website and Homebrew cask must be updated as part of the external release gate.</small>
</p>

<p align="center">
  🎬 <a href="https://www.youtube.com/watch?v=wfbCrfNASVU">Video intro (YouTube)</a>
  &nbsp;·&nbsp;
  <a href="https://www.bilibili.com/video/BV1845F6REUu/">视频介绍 (Bilibili)</a>
</p>

<p align="center">
  <a href="./README.zh-CN.md">中文说明</a>
  &nbsp;·&nbsp;
  <a href="https://x.com/JayTL00">@JayTL00 on X</a>
  &nbsp;·&nbsp;
  <a href="https://buymeacoffee.com/jaytl">Buy me a coffee</a>
</p>

<p align="center">
  <a href="https://trendshift.io/repositories/23290?utm_source=repository-badge&amp;utm_medium=badge&amp;utm_campaign=badge-repository-23290" target="_blank" rel="noopener noreferrer"><img src="https://trendshift.io/api/badge/repositories/23290" alt="xingkongliang%2Fskills-manager | Trendshift" width="250" height="55"/></a>
</p>

<p align="center">
  <a href="https://skills.sh/Zestful-ss/agentdock"><img src="https://skills.sh/b/Zestful-ss/agentdock" alt="manage-skills on skills.sh" /></a>
</p>

> The current V1.1 screenshots are being recaptured; the behavior documented below is authoritative.

## Features

- **Manage Agent skills and MCP; observe Harnesses** — Skills install only into the canonical roots (`~/.agents/skills`, `<repo>/.agents/skills`). Harnesses (Claude Code, Codex, Cursor, …) are discovery sources shown under **Inventory**; the app does not deploy into harness-specific folders.
- **Unified skill library** — Install skills from Git repos, local folders, or `.zip` / `.skill` archives into the canonical library. Metadata (SQLite, cache, logs) stays under `~/.skills-manager`.
- **My Skills** — Browse and curate the library; organize **presets** (curation groups) with membership toggles and ordering. Preset membership does not write harness files.
- **Inventory** — Read-only view of what each harness already discovers (skills + MCP), including native `.agents` consumers.
- **Project workspaces** — Manage project-local skills under `<repo>/.agents/skills` and compare them with the user library.
- **Add from Library sheet** — Open **+ Add Skills** to search the library and batch-add skills to a project.
- **Batch operations** — Multi-select skills for bulk enable/disable, update, or delete where the surface allows it.
- **Skill tagging and filters** — Tag skills, group by source or tag, and find untagged ones quickly.
- **Manual update tracking** — Check for upstream updates on Git-based skills; re-import local ones. Updates are never applied automatically.
- **Skill preview and source inspection** — Read `SKILL.md` / `README.md`, inspect source metadata, and compare local content with the upstream version inside the app.
- **Custom read-only paths** — Add extra Skill roots to Inventory. AgentDock observes them but never writes to them; use Adopt to copy a snapshot into a canonical library.
- **Activity log & Export Logs** — Install / remove / update operations are recorded locally. Use **Settings → Export Logs** to bundle recent logs and activity history into a single zip for easier issue reports.
- **Flexible app settings** — Configure discovery paths, theme, text size, language, proxy, diagnostics, and harness order — all in one place.

## Install

### macOS

The AgentDock Homebrew cask is being migrated with the first AgentDock release. Until it is published, download the `.dmg` for your Mac from the [latest release](https://github.com/Zestful-ss/agentdock/releases/latest).

### Windows and Linux

Download the installer for your platform from the [latest release](https://github.com/Zestful-ss/agentdock/releases/latest): `.exe` or `.msi` for Windows, and `.AppImage`, `.deb`, or `.rpm` for Linux (x64 and arm64).

Every installer includes the CLI for manual installation — see [Where the binary lives](#where-the-binary-lives).

## Quick Start

1. Install skills from local folders, Git repositories, or archives — they land in `~/.agents/skills` (user) or `<repo>/.agents/skills` (project).
2. Open **My Skills** to curate the library and organize presets (membership only).
3. Open **Inventory** to see what each harness already discovers (read-only).
4. For project-local skills, open a **Project** and manage `.agents/skills` there.
5. Configure discovery paths, custom read-only roots, theme, language, proxy, and diagnostics in **Settings**.
6. Use explicit **Adopt** when an external or Harness resource should become a managed snapshot; choose User or Project each time.

## Let your agents manage skills

Claude Code, Codex, Cursor and the rest can drive AgentDock through the [`manage-skills`](skills/manage-skills/SKILL.md) skill / CLI — install and curate the canonical library rather than writing into an agent folder behind its back. That keeps source metadata, preset membership, and update tracking intact.

It is also an ordinary published skill, so it can be installed without the app:

```bash
npx skills add Zestful-ss/agentdock
```

## Local-only operation

AgentDock is a local Windows-first manager. It does not provide Git backup, multi-device sync, an application updater, a tray/background lifecycle, or a local library export/import workflow. SQLite, tags, Presets, projects, and canonical Skills remain local; Harness and MCP resources are observed read-only.

## Observed harnesses

V1 discovery allowlist (Inventory / MCP observe these; the app does not write harness-specific skill dirs):

Claude Code · Codex · OpenCode · DeepSeek Harness · Kimi · Grok · Pi · Antigravity · Maka · OpenChamber

Native `.agents` consumers that pick up `~/.agents/skills` / `<repo>/.agents/skills` without a harness-specific deploy: Codex · OpenCode · DeepSeek Harness · Kimi · Pi.

**Settings** still lists harness paths for observation and custom discovery roots.

## Tech Stack

| Layer | Tech |
|-------|------|
| Frontend | React 19, TypeScript, Vite, Tailwind CSS |
| Desktop | Tauri 2 |
| Backend | Rust |
| Storage | SQLite (`rusqlite`) |
| i18n | react-i18next |

## Getting Started

### Prerequisites

- Node.js 20.19+ or 22.12+ (required by Vite 7)
- Rust 1.77.2 or newer
- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS

### Development

```bash
npm install
npm run tauri:dev
```

### CLI

The repository includes an agent-friendly CLI built on the same Rust shared core used by the desktop app. Both the CLI and the desktop app use the same canonical `.agents` roots and metadata index; Harness directories remain read-only discovery sources.

```bash
# Look around
npm run cli -- skills list
npm run cli -- skills show db

# Install into the library (canonical .agents/skills only — harnesses observe, never write)
npm run cli -- skills install ./my-skill
npm run cli -- skills install https://github.com/foo/bar/tree/main/skills/baz
npm run cli -- skills install vercel-labs/agent-skills@react-best-practices
npm run cli -- skills status react-best-practices

# Pull upstream changes, and adopt what an agent already has
npm run cli -- skills check --all
npm run cli -- skills update --all
npm run cli -- skills adopt ~/.claude/skills --dry-run
```

`--help` on any group or subcommand prints the full surface — the groups below
each carry more than these examples show, and destructive commands take
`--dry-run` (and `remove` requires `--yes`).

Available command groups:
- `repo` — inspect or change the configured base directory
- `agents` (`tools` alias) — list observed harnesses and include/exclude discovery sources
- `skills` — manage the canonical library (`list` / `show` / `install` / `update` / `remove` / `status` / tags / presets membership)
- `presets` — create, update, delete, organize, and inspect presets
- `inventory` — inspect Skills/MCP rows and manage custom read-only paths

Extra flags:
- `--skills-root <path>` — operate on an external canonical skills checkout instead of the local app default. The manager's state (DB, presets, cache, logs) lives in `~/.skills-manager/external/<name>-<hash>/`, namespaced by the canonical path of the skills root, so the external checkout itself stays clean.
- `--json` — machine-readable output for scripts/agents. Failures print `{"ok": false, "code": …, "message": …}` on stderr with a non-zero exit.

```bash
npm run -s cli -- --skills-root /path/to/my-skills --json skills list
```

#### Where the binary lives

The CLI is installed explicitly with `npm run cli:install` or from the standalone release asset. The app does not publish or refresh a CLI copy in the background. The legacy `skills-manager-cli` executable remains available as a compatibility alias.

Putting the CLI on your *own* PATH, for typing commands yourself, is separate:

```bash
npm run cli:install
# equivalent to:
# cargo install --path src-tauri --bin agentdock-cli --bin skills-manager-cli --locked --force
```

This installs the new binary at `~/.cargo/bin/agentdock-cli` and keeps the legacy `skills-manager-cli` name available. Re-run after pulling updates to refresh them.

Official releases publish `agentdock-cli-*` assets for macOS arm64/x64, Windows x64, and Linux x64/arm64. Download the matching asset, make it executable on macOS/Linux, and place it on PATH; legacy `skills-manager-cli-*` assets are also retained for compatibility.

#### Concurrent use with the desktop app

The CLI and desktop app share the same SQLite metadata index and repository lock. The desktop refreshes on navigation or explicit refresh; there is no background filesystem watcher.

### Build

```bash
npm run tauri:build
npm run cli:build
```

## Troubleshooting

**macOS refuses to open the app.** Releases from **v1.29.0** onward are signed with an Apple Developer ID certificate and notarized, so they open normally. If you see "Apple could not verify…" or "App is damaged", you are on v1.28.5 or older — upgrading is the fix.

Anything else — [open an issue](https://github.com/Zestful-ss/agentdock/issues), and attach the bundle from **Settings → Export Logs**.

## Star History

<p align="center">
  <a href="https://github.com/xingkongliang/star-history-svg">
    <img src="assets/star-history.svg" width="800" alt="Star History chart for Zestful-ss/agentdock" />
  </a>
</p>

## License

MIT
