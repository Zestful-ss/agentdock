import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { Download, FolderInput, Pencil, RefreshCw, Trash2, X } from "lucide-react";
import * as api from "../lib/tauri";
import { useCurrentProject } from "../lib/useCurrentProject";
import type {
  CanonicalScope,
  McpInventoryRow,
  MigrationEntry,
  SkillInventoryRow,
} from "../lib/tauri";
import { getErrorKind, getErrorMessage } from "../lib/error";

function statusLabel(status: SkillInventoryRow["status"]): string {
  switch (status) {
    case "managed":
      return "Managed";
    case "discovered":
      return "Discovered";
    case "system":
      return "System · Read-only";
    case "read_only":
      return "Read-only";
    case "conflict":
      return "Conflict";
    case "update_available":
      return "Update available";
    default:
      return status;
  }
}

function mcpStatusLabel(enabled: boolean | null): string {
  if (enabled === true) return "Enabled";
  if (enabled === false) return "Disabled";
  return "Configured";
}

export function Inventory() {
  const { projects, currentProject, selectProject } = useCurrentProject();
  const [panel, setPanel] = useState<"skills" | "mcp">("skills");
  const [skills, setSkills] = useState<SkillInventoryRow[]>([]);
  const [projectSkills, setProjectSkills] = useState<SkillInventoryRow[]>([]);
  const [mcp, setMcp] = useState<McpInventoryRow[]>([]);
  const [roots, setRoots] = useState<api.CanonicalRoots | null>(null);
  const [loading, setLoading] = useState(true);
  const [busyPath, setBusyPath] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [editing, setEditing] = useState<{
    name: string;
    scope: CanonicalScope;
    filename: string;
    content: string;
    saving: boolean;
  } | null>(null);
  const [migration, setMigration] = useState<MigrationEntry[] | null>(null);
  const [migrating, setMigrating] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [skillRows, mcpRows, canonical] = await Promise.all([
        api.getSkillInventory(),
        api.getMcpInventory(),
        api.getCanonicalRoots(),
      ]);
      setSkills(skillRows);
      setMcp(mcpRows);
      setRoots(canonical);
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, "Failed to load inventory"));
    } finally {
      setLoading(false);
    }
  }, []);

  const refreshProject = useCallback(async (projectId: string | null) => {
    if (!projectId) {
      setProjectSkills([]);
      return;
    }
    try {
      setProjectSkills(await api.getProjectSkillInventory(projectId));
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, "Failed to load project skills"));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    void refreshProject(currentProject?.id ?? null);
  }, [currentProject?.id, refreshProject]);

  const managed = useMemo(() => skills.filter((row) => row.status === "managed"), [skills]);
  const discovered = useMemo(
    () => skills.filter((row) => row.status !== "managed"),
    [skills],
  );
  const projectManaged = useMemo(
    () => projectSkills.filter((row) => row.status === "managed"),
    [projectSkills],
  );
  const projectDiscovered = useMemo(
    () => projectSkills.filter((row) => row.status !== "managed"),
    [projectSkills],
  );

  const adopt = async (row: SkillInventoryRow, target: "user" | "project", replace = false) => {
    if (row.system || row.read_only) return;
    if (target === "project" && !currentProject) {
      toast.error("Link a project workspace first");
      return;
    }
    setBusyPath(row.path);
    try {
      if (target === "user") {
        await api.adoptSkillToUser(row.path, replace);
        toast.success(`Imported ${row.name} to User ~/.agents/skills`);
      } else {
        await api.adoptSkillToProject(row.path, currentProject!.id, replace);
        toast.success(`Imported ${row.name} to Project .agents/skills`);
      }
      await refresh();
      await refreshProject(currentProject?.id ?? null);
    } catch (error: unknown) {
      if (!replace && getErrorKind(error) === "target_conflict") {
        toast.error(`"${row.name}" is already managed`, {
          action: {
            label: "Replace",
            onClick: () => void adopt(row, target, true),
          },
          duration: 8000,
        });
      } else {
        toast.error(getErrorMessage(error, "Import failed"));
      }
    } finally {
      setBusyPath(null);
    }
  };

  const remove = async (row: SkillInventoryRow, scope: CanonicalScope) => {
    const projectId = scope === "project" ? currentProject?.id ?? null : null;
    if (scope === "project" && !projectId) {
      toast.error("Link a project workspace first");
      return;
    }
    if (!window.confirm(`Delete "${row.name}" from ${scope === "user" ? "User ~/.agents/skills" : "Project .agents/skills"}?`)) {
      return;
    }
    setBusyPath(row.path);
    try {
      await api.deleteCanonicalSkill(row.name, scope, projectId);
      toast.success(`Deleted ${row.name}`);
      await refresh();
      await refreshProject(currentProject?.id ?? null);
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, "Delete failed"));
    } finally {
      setBusyPath(null);
    }
  };

  const openEditor = async (row: SkillInventoryRow, scope: CanonicalScope) => {
    const projectId = scope === "project" ? currentProject?.id ?? null : null;
    if (scope === "project" && !projectId) {
      toast.error("Link a project workspace first");
      return;
    }
    try {
      const doc = await api.readCanonicalSkillDocument(row.name, scope, projectId);
      setEditing({ name: row.name, scope, filename: doc.filename, content: doc.content, saving: false });
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, "Failed to open SKILL.md"));
    }
  };

  const saveEditor = async () => {
    if (!editing) return;
    const projectId = editing.scope === "project" ? currentProject?.id ?? null : null;
    setEditing((e) => (e ? { ...e, saving: true } : e));
    try {
      await api.saveCanonicalSkillDocument(editing.name, editing.content, editing.scope, projectId);
      toast.success(`Saved ${editing.filename}`);
      setEditing(null);
      await refresh();
      await refreshProject(currentProject?.id ?? null);
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, "Save failed"));
    } finally {
      setEditing((e) => (e ? { ...e, saving: false } : e));
    }
  };

  const runMigration = async () => {
    setMigrating(true);
    try {
      const report = await api.runLegacyMigration();
      setMigration(report);
      const conflicts = report.filter((r) => r.outcome === "conflict");
      const failed = report.filter((r) => r.outcome === "failed");
      if (report.length === 0) {
        toast.info("Nothing to migrate");
      } else if (conflicts.length > 0 || failed.length > 0) {
        toast.warning(
          `Migrated ${report.length - conflicts.length - failed.length}, ${conflicts.length} conflict(s), ${failed.length} failed`
        );
      } else {
        toast.success(`Migrated ${report.length} skill(s)`);
      }
      await refresh();
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, "Migration failed"));
    } finally {
      setMigrating(false);
    }
  };

  return (
    <div className="app-page gap-4">
      <div className="app-page-header border-b-0 pb-0">
        <div className="mb-3 flex items-start justify-between gap-3">
          <div>
            <h1 className="app-page-title">Skills / MCP</h1>
            <p className="app-page-subtitle text-tertiary">
              Manage <code className="text-secondary">.agents</code>, observe Harness.
            </p>
          </div>
          <div className="flex gap-1.5">
            <button
              onClick={() => void runMigration()}
              disabled={migrating}
              className="inline-flex items-center gap-1.5 rounded-md border border-border px-2.5 py-1.5 text-sm text-secondary hover:bg-surface-hover disabled:opacity-50"
              title="Migrate legacy ~/.skills-manager/skills into ~/.agents/skills (never deletes)"
            >
              {migrating ? "Migrating…" : "Migrate legacy"}
            </button>
            <button
              onClick={() => {
                void refresh();
                void refreshProject(currentProject?.id ?? null);
              }}
              className="inline-flex items-center gap-1.5 rounded-md border border-border px-2.5 py-1.5 text-sm text-secondary hover:bg-surface-hover"
            >
              <RefreshCw className="h-3.5 w-3.5" />
              Refresh
            </button>
          </div>
        </div>
        <div className="grid gap-2 text-sm text-tertiary md:grid-cols-2">
          <div className="rounded-md border border-border-subtle bg-surface px-3 py-2">
            <div className="text-[11px] font-semibold uppercase tracking-wide text-muted">User</div>
            <div className="truncate text-secondary">{roots?.user_skills ?? "…"}</div>
          </div>
          <div className="rounded-md border border-border-subtle bg-surface px-3 py-2">
            <div className="text-[11px] font-semibold uppercase tracking-wide text-muted">Project</div>
            {projects.length === 0 ? (
              <div className="truncate text-secondary">No project linked</div>
            ) : (
              <select
                value={currentProject?.id ?? ""}
                onChange={(e) => selectProject(e.target.value || null)}
                className="w-full bg-transparent text-secondary outline-none"
              >
                {projects.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name} — {p.path}\.agents\skills
                  </option>
                ))}
              </select>
            )}
          </div>
        </div>
        {migration !== null && (
          <div className="mt-2 rounded-md border border-border-subtle bg-surface px-3 py-2 text-xs text-secondary">
            {migration.length === 0 ? (
              <span>Legacy migration: nothing to migrate.</span>
            ) : (
              <ul className="space-y-0.5">
                {migration.map((m) => (
                  <li key={m.canonical_path || m.legacy_path}>
                    {m.name}: {m.outcome}
                    {m.outcome === "conflict" ? " (left untouched — resolve manually)" : ""}
                    {m.outcome === "failed" && m.error ? ` (failed: ${m.error})` : ""}
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
        <div className="mt-4 flex gap-1 border-b border-border-subtle">
          {[
            { id: "skills" as const, label: "Skills · Managed" },
            { id: "mcp" as const, label: "MCP · Discovered" },
          ].map((tab) => (
            <button
              key={tab.id}
              onClick={() => setPanel(tab.id)}
              className={`px-3 py-2 text-sm ${
                panel === tab.id
                  ? "border-b-2 border-accent text-primary"
                  : "text-muted hover:text-secondary"
              }`}
            >
              {tab.label}
            </button>
          ))}
        </div>
      </div>

      {loading ? (
        <p className="text-sm text-muted">Loading inventory…</p>
      ) : panel === "skills" ? (
        <div className="flex flex-col gap-6">
          <section>
            <h2 className="mb-2 text-sm font-semibold text-primary">User · Managed Skills</h2>
            {managed.length === 0 ? (
              <p className="text-sm text-muted">Nothing in ~/.agents/skills yet.</p>
            ) : (
              <div className="flex flex-col gap-2">
                {managed.map((row) => (
                  <SkillRow
                    key={row.path}
                    row={row}
                    busy={busyPath === row.path}
                    onEdit={() => void openEditor(row, "user")}
                    onDelete={() => void remove(row, "user")}
                  />
                ))}
              </div>
            )}
          </section>
          <section>
            <h2 className="mb-2 text-sm font-semibold text-primary">
              Project · Managed Skills{currentProject ? ` (${currentProject.name})` : ""}
            </h2>
            {!currentProject ? (
              <p className="text-sm text-muted">No project linked.</p>
            ) : projectManaged.length === 0 ? (
              <p className="text-sm text-muted">Nothing in this project's .agents/skills yet.</p>
            ) : (
              <div className="flex flex-col gap-2">
                {projectManaged.map((row) => (
                  <SkillRow
                    key={row.path}
                    row={row}
                    busy={busyPath === row.path}
                    onEdit={() => void openEditor(row, "project")}
                    onDelete={() => void remove(row, "project")}
                  />
                ))}
              </div>
            )}
          </section>
          {projectDiscovered.length > 0 && (
            <section>
              <h2 className="mb-2 text-sm font-semibold text-primary">Discovered in Project</h2>
              <div className="flex flex-col gap-2">
                {projectDiscovered.map((row) => (
                  <SkillRow
                    key={row.path}
                    row={row}
                    busy={busyPath === row.path}
                    onAdoptUser={() => void adopt(row, "user")}
                    onAdoptProject={() => void adopt(row, "project")}
                  />
                ))}
              </div>
            </section>
          )}
          <section>
            <h2 className="mb-2 text-sm font-semibold text-primary">Discovered Elsewhere (User)</h2>
            {discovered.length === 0 ? (
              <p className="text-sm text-muted">No harness-local skills found.</p>
            ) : (
              <div className="flex flex-col gap-2">
                {discovered.map((row) => (
                  <SkillRow
                    key={row.path}
                    row={row}
                    busy={busyPath === row.path}
                    onAdoptUser={() => void adopt(row, "user")}
                    onAdoptProject={() => void adopt(row, "project")}
                  />
                ))}
              </div>
            )}
          </section>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {mcp.length === 0 ? (
            <p className="text-sm text-muted">No MCP servers discovered.</p>
          ) : (
            mcp.map((row) => (
              <button
                key={row.name}
                onClick={() => setExpanded((cur) => (cur === row.name ? null : row.name))}
                className="rounded-md border border-border-subtle bg-surface px-3 py-2 text-left hover:bg-surface-hover"
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="font-medium text-primary">{row.name}</span>
                  <span className="text-[11px] uppercase text-muted">
                    {row.sources.filter((s) => s.configured).length} source(s)
                  </span>
                </div>
                <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-xs text-tertiary">
                  {row.sources
                    .filter((source) => source.configured)
                    .map((source) => (
                      <span key={source.harness}>
                        {source.display_name} · {source.transport} · {mcpStatusLabel(source.source_enabled)}
                      </span>
                    ))}
                </div>
                {expanded === row.name ? (
                  <div className="mt-2 space-y-1 text-xs text-secondary">
                    {row.sources
                      .filter((source) => source.configured)
                      .map((source) => (
                        <div key={`${source.harness}-path`}>
                          {source.display_name} · {source.transport} ·{" "}
                          {mcpStatusLabel(source.source_enabled)}
                          <div className="truncate text-tertiary">{source.source_path}</div>
                        </div>
                      ))}
                    <div className="text-muted">Read-only inventory. No enable/disable.</div>
                  </div>
                ) : null}
              </button>
            ))
          )}
        </div>
      )}

      {editing && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
          <div className="flex max-h-[85vh] w-full max-w-2xl flex-col rounded-lg border border-border bg-surface">
            <div className="flex items-center justify-between border-b border-border-subtle px-4 py-2.5">
              <div className="text-sm font-semibold text-primary">
                {editing.name} · {editing.filename} ·{" "}
                {editing.scope === "user" ? "User" : "Project"}
              </div>
              <button
                onClick={() => setEditing(null)}
                className="rounded p-1 text-muted hover:bg-surface-hover hover:text-secondary"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
            <textarea
              value={editing.content}
              onChange={(e) => setEditing((prev) => (prev ? { ...prev, content: e.target.value } : prev))}
              spellCheck={false}
              className="min-h-[300px] flex-1 resize-y bg-background p-3 font-mono text-xs text-primary outline-none"
            />
            <div className="flex items-center justify-end gap-2 border-t border-border-subtle px-4 py-2.5">
              <button
                onClick={() => setEditing(null)}
                className="rounded border border-border px-3 py-1.5 text-sm text-secondary hover:bg-surface-hover"
              >
                Cancel
              </button>
              <button
                onClick={() => void saveEditor()}
                disabled={editing.saving}
                className="rounded border border-accent-border bg-accent-dark px-3 py-1.5 text-sm text-white hover:bg-accent disabled:opacity-50"
              >
                {editing.saving ? "Saving…" : "Save"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function SkillRow({
  row,
  busy,
  onAdoptUser,
  onAdoptProject,
  onEdit,
  onDelete,
}: {
  row: SkillInventoryRow;
  busy?: boolean;
  onAdoptUser?: () => void;
  onAdoptProject?: () => void;
  onEdit?: () => void;
  onDelete?: () => void;
}) {
  return (
    <div className="rounded-md border border-border-subtle bg-surface px-3 py-2">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-medium text-primary">{row.name}</span>
            <span className="rounded bg-bg-secondary px-1.5 py-0.5 text-[11px] text-muted">
              {statusLabel(row.status)}
            </span>
            <span className="text-[11px] text-muted">{row.source_display_name}</span>
          </div>
          <div className="truncate text-xs text-tertiary">{row.path}</div>
          {row.native_consumers.length > 0 ? (
            <div className="mt-1 text-[11px] text-muted">
              Native consumers: {row.native_consumers.join(", ")}
            </div>
          ) : row.status === "managed" ? (
            <div className="mt-1 text-[11px] text-muted">Not a native consumer for other harnesses</div>
          ) : null}
        </div>
        <div className="flex shrink-0 gap-1">
          {onAdoptUser && !row.system && (
            <>
              <button
                disabled={busy}
                onClick={onAdoptUser}
                className="inline-flex items-center gap-1 rounded border border-border px-2 py-1 text-xs text-secondary hover:bg-surface-hover disabled:opacity-50"
              >
                <Download className="h-3 w-3" />
                User
              </button>
              <button
                disabled={busy}
                onClick={onAdoptProject}
                className="inline-flex items-center gap-1 rounded border border-border px-2 py-1 text-xs text-secondary hover:bg-surface-hover disabled:opacity-50"
              >
                <FolderInput className="h-3 w-3" />
                Project
              </button>
            </>
          )}
          {onEdit && (
            <button
              disabled={busy}
              onClick={onEdit}
              className="inline-flex items-center gap-1 rounded border border-border px-2 py-1 text-xs text-secondary hover:bg-surface-hover disabled:opacity-50"
            >
              <Pencil className="h-3 w-3" />
              Edit
            </button>
          )}
          {onDelete && (
            <button
              disabled={busy}
              onClick={onDelete}
              className="inline-flex items-center gap-1 rounded border border-border px-2 py-1 text-xs text-secondary hover:bg-surface-hover disabled:opacity-50"
            >
              <Trash2 className="h-3 w-3" />
              Delete
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
