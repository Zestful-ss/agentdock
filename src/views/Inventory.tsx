import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { Download, FolderInput, Pencil, RefreshCw, Trash2, X } from "lucide-react";
import * as api from "../lib/tauri";
import { useCurrentProject } from "../lib/useCurrentProject";
import type {
  AdoptDiff,
  CanonicalScope,
  McpInventoryRow,
  MigrationEntry,
  SkillInventoryRow,
} from "../lib/tauri";
import { getErrorKind, getErrorMessage } from "../lib/error";
import { DocumentDiffViewer } from "../components/DocumentDiffViewer";

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
  const [adoptDiff, setAdoptDiff] = useState<{
    row: SkillInventoryRow;
    target: "user" | "project";
    diff: AdoptDiff;
  } | null>(null);
  const [visibility, setVisibility] = useState<"active" | "ignored" | "hidden" | "all">("active");

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

  const matchesVisibility = useCallback(
    (row: { ignored: boolean; hidden: boolean }) => {
      if (visibility === "all") return true;
      if (visibility === "ignored") return row.ignored;
      if (visibility === "hidden") return row.hidden;
      return !row.ignored && !row.hidden;
    },
    [visibility],
  );

  const visibleSkills = useMemo(
    () => skills.filter(matchesVisibility),
    [matchesVisibility, skills],
  );
  const visibleProjectSkills = useMemo(
    () => projectSkills.filter(matchesVisibility),
    [matchesVisibility, projectSkills],
  );
  const visibleMcp = useMemo(
    () => mcp.filter(matchesVisibility),
    [matchesVisibility, mcp],
  );

  const managed = useMemo(
    () => visibleSkills.filter((row) => row.status === "managed"),
    [visibleSkills],
  );
  const discovered = useMemo(
    () => visibleSkills.filter((row) => row.status !== "managed"),
    [visibleSkills],
  );
  const projectManaged = useMemo(
    () => visibleProjectSkills.filter((row) => row.status === "managed"),
    [visibleProjectSkills],
  );
  const projectDiscovered = useMemo(
    () => visibleProjectSkills.filter((row) => row.status !== "managed"),
    [visibleProjectSkills],
  );

  const updateResourceState = async (
    kind: "skill" | "mcp",
    path: string,
    state: Partial<api.InventoryResourceState>,
  ) => {
    try {
      await api.setInventoryResourceState(kind, path, state);
      await refresh();
      await refreshProject(currentProject?.id ?? null);
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, "Failed to update resource state"));
    }
  };

  const adopt = async (row: SkillInventoryRow, target: "user" | "project", replace = false) => {
    if (row.system) return;
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
        try {
          const diff = await api.getAdoptDiff(
            row.path,
            row.name,
            target,
            currentProject?.id ?? null,
          );
          setAdoptDiff({ row, target, diff });
        } catch (diffError: unknown) {
          toast.error(getErrorMessage(diffError, "Failed to compare the existing skill"));
        }
      } else {
        toast.error(getErrorMessage(error, "Adopt failed"));
      }
    } finally {
      setBusyPath(null);
    }
  };

  const confirmAdoptReplace = async () => {
    if (!adoptDiff) return;
    const { row, target } = adoptDiff;
    setAdoptDiff(null);
    await adopt(row, target, true);
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
        <div className="mt-4 flex items-center justify-between gap-3 border-b border-border-subtle">
          <div className="flex gap-1">
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
          <label className="flex items-center gap-2 pr-1 text-xs text-muted">
            <span>Show</span>
            <select
              value={visibility}
              onChange={(e) => setVisibility(e.target.value as typeof visibility)}
              className="rounded border border-border-subtle bg-surface px-1.5 py-1 text-xs text-secondary outline-none"
            >
              <option value="active">Visible</option>
              <option value="ignored">Ignored</option>
              <option value="hidden">Hidden</option>
              <option value="all">All</option>
            </select>
          </label>
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
                     onToggleIgnored={() => void updateResourceState("skill", row.path, { ignored: !row.ignored })}
                     onToggleHidden={() => void updateResourceState("skill", row.path, { hidden: !row.hidden })}
                     onEditNote={() => {
                       const note = window.prompt("Resource note", row.note);
                       if (note !== null) void updateResourceState("skill", row.path, { note });
                     }}
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
                     onToggleIgnored={() => void updateResourceState("skill", row.path, { ignored: !row.ignored })}
                     onToggleHidden={() => void updateResourceState("skill", row.path, { hidden: !row.hidden })}
                     onEditNote={() => {
                       const note = window.prompt("Resource note", row.note);
                       if (note !== null) void updateResourceState("skill", row.path, { note });
                     }}
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
                     onToggleIgnored={() => void updateResourceState("skill", row.path, { ignored: !row.ignored })}
                     onToggleHidden={() => void updateResourceState("skill", row.path, { hidden: !row.hidden })}
                     onEditNote={() => {
                       const note = window.prompt("Resource note", row.note);
                       if (note !== null) void updateResourceState("skill", row.path, { note });
                     }}
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
                     onToggleIgnored={() => void updateResourceState("skill", row.path, { ignored: !row.ignored })}
                     onToggleHidden={() => void updateResourceState("skill", row.path, { hidden: !row.hidden })}
                     onEditNote={() => {
                       const note = window.prompt("Resource note", row.note);
                       if (note !== null) void updateResourceState("skill", row.path, { note });
                     }}
                  />
                ))}
              </div>
            )}
          </section>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {visibleMcp.length === 0 ? (
            <p className="text-sm text-muted">No MCP servers discovered.</p>
          ) : (
            visibleMcp.map((row) => (
              <div key={row.id} className="rounded-md border border-border-subtle bg-surface p-1">
                <button
                  onClick={() => setExpanded((cur) => (cur === row.id ? null : row.id))}
                  className="w-full rounded-md px-3 py-2 text-left hover:bg-surface-hover"
                >
                <div className="flex items-center justify-between gap-2">
                  <span className="font-medium text-primary">{row.name}</span>
                  {row.ignored && <span className="ml-2 text-[11px] text-amber-600">Ignored</span>}
                  {row.hidden && <span className="ml-2 text-[11px] text-slate-500">Hidden</span>}
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
                {expanded === row.id ? (
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
                    {row.note && <div className="italic text-muted">Note: {row.note}</div>}
                    <div className="text-muted">Read-only inventory. No enable/disable.</div>
                  </div>
                ) : null}
              </button>
                <div className="flex items-center justify-end gap-1 border-t border-border-subtle px-2 pt-1">
                  <button
                    onClick={() => void updateResourceState("mcp", row.id, { ignored: !row.ignored })}
                    className="rounded border border-border px-2 py-1 text-[11px] text-secondary hover:bg-surface-hover"
                  >
                    {row.ignored ? "Show" : "Ignore"}
                  </button>
                  <button
                    onClick={() => void updateResourceState("mcp", row.id, { hidden: !row.hidden })}
                    className="rounded border border-border px-2 py-1 text-[11px] text-secondary hover:bg-surface-hover"
                  >
                    {row.hidden ? "Unhide" : "Hide"}
                  </button>
                </div>
              </div>
            ))
          )}
        </div>
      )}

      {adoptDiff && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
          <div className="flex max-h-[85vh] w-full max-w-5xl flex-col rounded-lg border border-border bg-surface">
            <div className="flex items-center justify-between border-b border-border-subtle px-4 py-2.5">
              <div className="text-sm font-semibold text-primary">
                Review Adopt replacement · {adoptDiff.row.name} · {adoptDiff.target}
              </div>
              <button
                onClick={() => setAdoptDiff(null)}
                className="rounded p-1 text-muted hover:bg-surface-hover hover:text-secondary"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
            <div className="min-h-0 flex-1 overflow-auto p-3">
              <DocumentDiffViewer
                original={adoptDiff.diff.original}
                updated={adoptDiff.diff.updated}
              />
              <div className="mt-2 text-[11px] text-muted">
                Existing: {adoptDiff.diff.target_path} · Source: {adoptDiff.diff.source_path}
              </div>
            </div>
            <div className="flex items-center justify-end gap-2 border-t border-border-subtle px-4 py-2.5">
              <button
                onClick={() => setAdoptDiff(null)}
                className="rounded border border-border px-3 py-1.5 text-sm text-secondary hover:bg-surface-hover"
              >
                Cancel
              </button>
              <button
                onClick={() => void confirmAdoptReplace()}
                className="rounded border border-accent-border bg-accent-dark px-3 py-1.5 text-sm text-white hover:bg-accent"
              >
                Replace
              </button>
            </div>
          </div>
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
  onToggleIgnored,
  onToggleHidden,
  onEditNote,
}: {
  row: SkillInventoryRow;
  busy?: boolean;
  onAdoptUser?: () => void;
  onAdoptProject?: () => void;
  onEdit?: () => void;
  onDelete?: () => void;
  onToggleIgnored?: () => void;
  onToggleHidden?: () => void;
  onEditNote?: () => void;
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
            <span className="rounded bg-bg-secondary px-1.5 py-0.5 text-[11px] text-muted">
              {row.source_kind} · {row.ownership}
            </span>
            <span className="text-[11px] text-muted">{row.source_display_name}</span>
             {row.ignored && (
               <span className="rounded bg-amber-500/12 px-1.5 py-0.5 text-[11px] text-amber-600 dark:text-amber-400">Ignored</span>
             )}
             {row.hidden && (
               <span className="rounded bg-slate-500/12 px-1.5 py-0.5 text-[11px] text-slate-500">Hidden</span>
             )}
          </div>
          <div className="truncate text-xs text-tertiary">{row.path}</div>
           {row.note && <div className="mt-1 text-[11px] italic text-muted">Note: {row.note}</div>}
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
          {onToggleIgnored && (
             <button
               disabled={busy}
               onClick={onToggleIgnored}
               className="rounded border border-border px-2 py-1 text-xs text-secondary hover:bg-surface-hover disabled:opacity-50"
             >
               {row.ignored ? "Show" : "Ignore"}
             </button>
           )}
           {onToggleHidden && (
             <button
               disabled={busy}
               onClick={onToggleHidden}
               className="rounded border border-border px-2 py-1 text-xs text-secondary hover:bg-surface-hover disabled:opacity-50"
             >
               {row.hidden ? "Unhide" : "Hide"}
             </button>
           )}
           {onEditNote && (
             <button
               disabled={busy}
               onClick={onEditNote}
               className="rounded border border-border px-2 py-1 text-xs text-secondary hover:bg-surface-hover disabled:opacity-50"
             >
               Note
             </button>
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
