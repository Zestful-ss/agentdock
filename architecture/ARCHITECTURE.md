# V1 architecture inventory

Product: **manage `.agents`, observe Harness.**

Harnesses are discovery sources, not deployment targets. V1 writes only:

```
USER     %USERPROFILE%\.agents\skills
PROJECT  <repo>\.agents\skills
```

App metadata (SQLite, cache, logs) stays in `~/.skills-manager`. That is not a skill library.

## KEEP

| Area | Path | Why |
|---|---|---|
| SkillStore | `core/skill_store.rs` | Managed skill records, tags, discovered rows |
| Git fetch / preview | `core/git_fetcher.rs`, `commands/skills.rs` preview/confirm | Source → catalog → install by subpath already exists |
| GitHub API | `core/github_api.rs` | Remote metadata |
| Update check | `core/skill_auto_updater.rs`, `check_skill_update` | Subpath-aware updates already recorded |
| Installer | `core/installer.rs` | Copy a SKILL.md tree into a destination |
| Scanner | `core/scanner.rs` | Read-only SKILL.md walk |
| Project scanner | `core/project_scanner.rs` | Project-local discovery |
| Content hash | `core/content_hash.rs` | Identity / update |
| Path guard | `core/path_guard.rs` | Safety |
| Settings / diagnostics | `commands/settings.rs` | App settings, not harness writes |

## ADAPT

| Area | Change |
|---|---|
| `central_repo::skills_dir` | Point at `~/.agents/skills`, not `~/.skills-manager/skills` |
| `tool_adapters.rs` | Allowlist V1 harnesses; treat adapters as discovery sources |
| Project export | Only write `<repo>/.agents/skills` |
| Scan / import | Adopt into User or Project `.agents`, never into `.codex` / `.dsh` |
| UI workspaces | User + Project + MCP inventory instead of 50-agent Global Workspace |
| Git install | Keep Source/Catalog/Skill split (`source_subpath` already stored) |

## HIDE (leave code, cut UI / command entry)

| Area | How |
|---|---|
| Harness copy/symlink deploy | `sync_skill_to_tool`, `unsync_skill_from_tool`, `set_skill_tool_toggle` removed from the invoke handler; remaining writes that still name a harness path return `v1::blocked_write()` |
| Preset apply to agents | `apply_preset_to_default`, `switch_preset`, `apply_preset_to_coding_agents` removed (P2.3); tray apply / schedule entries removed with them |
| Project export to harness dirs | `export_skill_to_project`, `toggle_project_skill`, `delete_project_skill` blocked unless dest is `.agents/skills` (`toggle_project_skill` always returns `blocked_write`) |
| Delete harness-local skill | `delete_global_local_skill` returns `v1::blocked_write()` |
| skills.sh marketplace UI | Install tab Market block removed (P2.1); CLI `skills search` / skillssh install remain |
| Multi-device backup UI | Backup nav / Backup page removed (P2.1); git backup commands stay registered (Settings + FirstRun restore) |
| Lobster / Global Workspace | WorkspaceView removed (P2.1); sidebar routes are Inventory / My Skills / Install / Project / Settings |

## REMOVE later (not this round)

Physical deletion of `sync_engine` harness deploy paths, presets disk-sync, marketplace client, and unused git backup surface. Do not delete until MCP inventory + `.agents` install are proven, or Git import / update tracking can break. P2 already removed: `commands/browse.rs`, `commands/sync.rs`, `apply_preset_from_tray` / tray schedule, `installer::install_from_git_dir`, and the dead `git_backup` / `scanner` / `tool_adapters` / `sync_metadata` / `agent_workspace` helpers that only those paths used.

## V1 discovery allowlist

Only GitHub-found open source harnesses, plus Maka / OpenChamber (local config known):

| id | repo / notes |
|---|---|
| `codex` | https://github.com/openai/codex |
| `claude_code` | https://github.com/anthropics/claude-code |
| `opencode` | https://github.com/anomalyco/opencode |
| `deepseek_harness` | https://github.com/deepseek-ai/deepseek-harness |
| `kimi` | https://github.com/MoonshotAI/kimi-code |
| `grok` | https://github.com/xai-org/grok-build |
| `pi` | https://github.com/earendil-works/pi (派) |
| `antigravity` | https://github.com/google-antigravity/antigravity-cli |
| `maka` | local AppData config, discovery only |
| `openchamber` | local `%USERPROFILE%\.config\openchamber`, discovery only |

**Not in V1:** Z Code ADE (no open harness; only https://github.com/zai-org/zcode-plugins).

## Skill status

`Managed` | `Discovered` | `Read-only` | `System` | `Conflict` | `Update available`

Same name, different path → two rows. Never collapse.

## MCP V1

Inventory only. Transports: `stdio` | `streamable-http` | `legacy-sse` | `unknown`. Keep `raw_config`. No enable/disable. Pin/Hide/Ignore/Tag may live in manager settings later.
