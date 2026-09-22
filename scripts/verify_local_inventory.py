"""Read-only V1 inventory check against this Windows machine.

Does not write harness configs. Prints Managed skills, Discovered skills,
and MCP inventory rows for OpenCode / Maka / Codex.
"""
from __future__ import annotations

import json
from pathlib import Path

home = Path.home()
user_skills = home / ".agents" / "skills"


def skill_dirs(root: Path) -> list[Path]:
    if not root.is_dir():
        return []
    found = []
    for child in root.iterdir():
        if child.is_dir() and ((child / "SKILL.md").exists() or (child / "skill.md").exists()):
            found.append(child)
    return found


print("USER", user_skills)
print("MANAGED")
for path in skill_dirs(user_skills):
    print(" ", path.name, path)

print("DISCOVERED")
for root in [
    home / ".codex" / "skills",
    home / ".codex" / "skills" / ".system",
    home / ".dsh" / "skills",
    home / ".config" / "opencode" / "skills",
]:
    for path in skill_dirs(root):
        kind = "system" if ".system" in path.parts else "discovered"
        print(" ", kind, path.name, path)

mcp_files = [
    ("opencode", home / ".config" / "opencode" / "opencode.json"),
    ("maka", Path.home() / "AppData" / "Roaming" / "Maka" / "workspaces" / "default" / "mcp.json"),
]
print("MCP")
for harness, path in mcp_files:
    if not path.exists():
        print(" ", harness, "missing", path)
        continue
    data = json.loads(path.read_text(encoding="utf-8"))
    servers = data.get("mcp") or data.get("mcpServers") or {}
    for name, cfg in servers.items():
        enabled = cfg.get("enabled")
        kind = cfg.get("type") or ("url" if "url" in cfg else "command")
        print(f"  {name:22} {harness:10} enabled={enabled} transport={kind}")
