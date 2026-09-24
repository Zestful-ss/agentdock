---
name: manage-skills
description: Manage the user's canonical agent-skill library (~/.agents/skills and <repo>/.agents/skills) via agentdock-cli — install, update, remove, curate presets, organize tags, search, and adopt existing skills. Use this whenever the user wants a skill added or removed from the library, wants presets/tags organized, or asks what is installed. Harnesses are discovery sources only; do not write into harness-specific folders.
---

## Before doing anything

1. **Resolve the CLI first, then use the path it prints.** AgentDock does not
   publish a background bridge copy. Prefer the explicitly installed binary:

   ```bash
   P="$(command -v agentdock-cli 2>/dev/null || command -v skills-manager-cli 2>/dev/null || true)"
   [ -x "$P" ] && echo "$P"
   ```

   **Substitute the printed path into every command below**, wherever the
   examples write `$SM`. Do not carry `$SM` as a shell variable: each command
   you run is a new shell, so an assignment made here is gone by the next one.

   If nothing is printed, ask the user to install the standalone CLI with
   `npm run cli:install` or place the release asset on PATH. Do not search for
   or execute an unverified binary under the legacy metadata directory.
2. **Always pass `--json` when you parse output yourself.** Pretty-printed output is for the user; JSON is for you. Errors include `ok=false`, a stable `code`, and `message` on stderr with a non-zero exit code.

```bash
"$SM" --json skills list
```

## Mental model

V1 writes skills only into the canonical roots — user `~/.agents/skills/` and project `<repo>/.agents/skills/` — not into harness-specific folders. Harnesses are discovery sources (Inventory / MCP observe them); they are not deployment targets. Each skill has source metadata, preset membership, tags, and a canonical location. A **preset** is a reusable curation group; several presets may be members at the same time.

Keep these three states separate:
- **Library**: install/remove controls whether AgentDock owns the skill under `.agents`.
- **Preset membership**: `presets add-skill/remove-skill` organizes the library only (curation, not disk sync).
- **Harness observation**: Inventory / agents list report what a harness already sees; they never write into harness dirs.

Internally, presets are still stored as scenarios for SQLite/schema compatibility. The CLI and UI call them presets; they are curation metadata only.

## Install

```bash
# From a skills.sh-compatible GitHub source
"$SM" skills install vercel-labs/agent-skills@react-best-practices

# Any git URL (use /tree/branch/subpath form when the skill lives in a sub-directory)
"$SM" skills install https://github.com/anthropics/skills.git
"$SM" skills install https://github.com/foo/bar/tree/main/skills/baz

# Local folder
"$SM" skills install ./my-skill

# Force a source type when the ref is ambiguous
"$SM" skills install foo/bar --skillssh
"$SM" skills install ./looks-like/owner-repo --local
```

**Default is library-only** — the skill enters the canonical library under `~/.agents/skills`. Native `.agents` consumers (Codex, OpenCode, DeepSeek Harness, Kimi, Pi) pick it up from there. Do not deploy into harness-specific folders in V1.

**Ref resolution** is deterministic, no path-existence guessing:
1. Starts with `./`, `../`, `/`, or `~/` → local path
2. Contains `://`, ends in `.git`, or starts with `git@` → git URL
3. Matches `owner/repo`, `owner/repo/skill`, or `owner/repo@skill` → skillssh
4. Otherwise → error; pass `--local` / `--git` / `--skillssh` to disambiguate

**Always verify after install** with `skills list` or `skills show <name>` so you can confirm the skill landed and report its canonical path and preset membership.

## Search

```bash
"$SM" --json skills search "react performance" --limit 5
```

Each result has `install_ref` (paste straight into `skills install`), `installs` (popularity proxy), and `skills_sh_url`. Show the top 1–3 with install counts before installing — anything with 10K+ installs is battle-tested; anything under 100 needs a careful look at the source repo.

## Update / Check

```bash
# Re-fetch one skill (git/skillssh re-clones, local/import re-imports source dir)
"$SM" skills update <skill-name-or-id>

# Re-fetch all eligible skills
"$SM" skills update --all

# Just probe remote revisions, don't touch files
"$SM" skills check --all
```

`check` is the dry-run partner of `update`. Local-only skills (no git source) are reported as `skipped: true`.

**An update replaces the skill's directory wholesale**, so anything written inside it that the new version does not have would be destroyed. When the CLI detects that, it applies nothing and reports the paths instead:

```jsonc
{ "name": "ppt-master", "refreshed": false,
  "held_back_removals": ["library: templates/mine.pptx"] }
```

The field is omitted entirely when nothing is held back, so test for its presence rather than for an empty array. `refreshed: false` *with* `held_back_removals` is **not a failure and not something to retry** — the skill is untouched and still on its old version. Show the user the listed paths and ask. There is no CLI flag to override this; only the desktop app can confirm and proceed, because only a person can say those files are expendable. The paths are relative to the canonical library.

Note that the update path also compares the live canonical directory with the indexed hash. If the managed copy was edited locally, the update stops with a conflict instead of overwriting that edit.

## Remove

```bash
# Always preview first when removing more than one
"$SM" skills remove <skill> --dry-run

# --yes is required for the actual delete; --json mode does NOT auto-confirm
"$SM" skills remove <skill> --yes
```

Remove deletes the canonical library directory and its metadata row. Legacy Harness target rows are removed from the index only; Harness files are never touched.

## Observe discovery state (read-only)

```bash
"$SM" --json skills status <skill>
```

`skills status` reports the local library record and discovered agent availability. It does not report or filter by deployment projections; Harness directories are observe-only in V1 and there is no CLI path that writes them.

## Inventory

```bash
"$SM" --json inventory skills
"$SM" --json inventory mcp
"$SM" --json inventory paths list
"$SM" --json inventory paths add C:\\Tools\\shared-skills
"$SM" --json inventory paths remove C:\\Tools\\shared-skills --dry-run
"$SM" --json inventory state --kind skill C:\\Users\\me\\.claude\\skills\\my-skill --ignored true
# MCP state uses the row id, so different servers sharing one config stay independent
"$SM" --json inventory state --kind mcp opencode:github:C:\\Users\\me\\AppData\\Roaming\\opencode\\settings.json --ignored true
```

Inventory rows retain source and ownership. Duplicate names from different
Harnesses are separate resources; MCP rows never expose commands, arguments,
environment variables, headers, or raw config. Custom paths are read-only
until an explicit Adopt copies a snapshot into a canonical library.

## Adopt skills installed elsewhere

When skills already live in a Harness directory (e.g. installed via `npx skills add` or a manual `git clone`) but aren't in the canonical library, adopt them:

```bash
# Dry-run scan first — lists candidates without writing
"$SM" skills adopt ~/.claude/skills --dry-run

# Adopt everything found — each becomes source_type=local (can't auto-update from git)
"$SM" skills adopt ~/.claude/skills

# Adopt a single skill and pin it to a git source so `update` works later
"$SM" skills adopt ~/.claude/skills/react-best-practices \
  --git-url https://github.com/vercel-labs/agent-skills/tree/main/react-best-practices

# Or pass --git-subpath explicitly when the URL is just the repo root
"$SM" skills adopt ~/.claude/skills/react-best-practices \
  --git-url https://github.com/vercel-labs/agent-skills \
  --git-subpath react-best-practices

# Skill lives at the repo root? Pass an empty subpath
"$SM" skills adopt ~/.claude/skills/my-skill \
  --git-url https://github.com/me/my-skill --git-subpath ""
```

`adopt` is safe to re-run: already managed canonical paths are excluded, while the discovered source directory remains untouched. `--git-url` requires either a URL with a subpath (`/tree/branch/path`) or an explicit `--git-subpath` — without that, future `update` would re-clone the wrong directory, so the CLI refuses to guess.

`--git-url` only applies at the moment of adoption, while the directory is still unmanaged. Once a skill is in the library, use `set-source` below.

## Re-point a skill at a git source

```bash
# Preview: resolves the source and reports whether content differs. It clones to
# a temp dir, but writes nothing to the library or the DB.
"$SM" --json skills set-source <skill> --git-url you/skills --subpath my-skill --dry-run

# A GitHub /tree/ URL carries the branch and subpath already
"$SM" skills set-source <skill> --git-url https://github.com/you/skills/tree/main/my-skill
```

This is how a `local` skill becomes git-backed so `update` works, and how a
skill pointed at the wrong repo gets corrected. It updates the row **in place**,
so the skill id survives and the tags and preset membership keyed to it all stay intact.

- The flag is `--subpath` here, not `--git-subpath` — that one belongs to `adopt`. Pass `--subpath ""` when the skill is at the repo root, which must itself hold a `SKILL.md`.
- `--branch` overrides a branch encoded in the URL.
- The report carries `content_changed` — a single boolean, **not** a file list. A replacement also verifies the live canonical hash; if the managed copy was edited locally, it refuses to apply the update until the conflict is resolved.

**`--force` is destructive, and nothing stands between it and the user's files.**
A content difference is refused without it. With it, the whole skill directory is
replaced — staged, swapped in, and the old copy deleted — so anything in the
library copy that the new source does not ship is gone. Unlike `skills update`,
this path has **no** `held_back_removals` check: nothing is withheld, and nothing
asks. `--dry-run` cannot tell you which files are at stake, only that something
differs. Never pass `--force` on the user's behalf — report `content_changed:
true`, say that proceeding overwrites the library copy wholesale, and let them
decide.

## Tag

```bash
"$SM" skills tag add <skill> web frontend
"$SM" skills tag remove <skill> frontend
"$SM" skills tag set <skill> web frontend
"$SM" skills tag rename frontend web
"$SM" skills tag delete obsolete --dry-run
"$SM" skills tag delete obsolete --yes
"$SM" skills tag list <skill>   # tags on one skill
"$SM" skills tag list           # all distinct tags
```

Useful organization queries:

```bash
"$SM" --json skills list --untagged
"$SM" --json skills list --no-preset
"$SM" --json skills list --tag frontend
"$SM" --json skills list --preset "Web Dev"
```

## Presets

```bash
"$SM" presets list
"$SM" presets current
"$SM" presets show "Web Dev"
"$SM" presets create "Web Dev" --description "Frontend work"
"$SM" presets update "Web Dev" --name "Frontend"
"$SM" presets delete "Old" --dry-run
"$SM" presets delete "Old" --yes

"$SM" presets add-skill <preset> <skill>...
"$SM" presets remove-skill <preset> <skill>...
"$SM" --json presets status <preset>
```

Preset create/update/delete and add-skill/remove-skill are organization-only CLI operations. They never write harness files.

## Health check

When a command errors in a confusing way:

```bash
"$SM" --json repo status   # base dir, skill / preset counts, active preset
"$SM" --json agents list  # detected agents and their observed paths
"$SM" agents enable codex
"$SM" agents disable claude_code
```

`repo status` and `agents list` are read-only and are the first checks for "why isn't this skill showing up in Cursor" questions. `agents disable` / `enable` only flip the agent's registration flag for discovery; they do not write harness directories.

## Typical workflows

### "Find me a skill for X" / "Install a skill that does X"

1. `skills search "X" --limit 5` — show the top 1–3 hits with install counts and source.
2. If a clear winner: `skills install <install_ref>`.
3. If ambiguous: ask the user to pick.
4. `skills status <name>` to confirm the library state.

### "What skills do I have?"

```bash
"$SM" --json skills list
```

The `preset_ids`, `presets`, `tags`, and `source_type` fields are usually the most informative. The legacy `enabled` field is not deployment state.

### "Pull in the skills already installed in my agent directories"

1. `skills adopt ~/.claude/skills --dry-run` (and any other agent dirs the user mentions) — show the candidate list.
2. After user confirms: `skills adopt ~/.claude/skills`.
3. For any adopted skill where the user knows the original repo, restore the update link with `skills set-source <skill> --git-url ... --subpath ...`.

### "Update everything"

```bash
"$SM" skills check --all     # see what has upstream changes
"$SM" skills update --all    # apply
```

Report which skills actually refreshed (`refreshed: true` in the JSON) vs which were already up-to-date.

## Pitfalls

- **Install succeeded but a non-native harness doesn't show it** → V1 only writes `.agents`; harnesses that do not read that root must be observed under Inventory, not written to. Prefer native consumers or ask the user to point their harness at `.agents/skills`.
- **Preset membership changed but disk content did not** → membership is organization only. Installing into `.agents` is what makes content visible to native consumers.
- **Adopted skills can't be `update`d from git** → `npx skills add` and manual `git clone` don't leave source metadata, so adopt has to treat them as `local`. Re-point them with `skills set-source`. Do **not** reach for `adopt --git-url` here: adopt only ever creates new library entries, and it fails *late* — `--dry-run` returns `ok: true` with the skill sitting in `skipped`, and only the real run errors with `--git-url requires exactly one adoptable skill, found 0`. Do **not** remove-then-reinstall either — that drops the skill id, and with it the tags and preset membership.
- Use `--dry-run` before bulk remove, tag delete, or preset delete operations. Use `check` before `update`.
