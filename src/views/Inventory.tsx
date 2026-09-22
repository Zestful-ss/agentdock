import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { Download, FolderInput, RefreshCw } from "lucide-react";
import { useApp } from "../context/AppContext";
import * as api from "../lib/tauri";
import type { McpInventoryRow, SkillInventoryRow } from "../lib/tauri";
import { getErrorMessage } from "../lib/error";

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

export function Inventory() {
  const { projects } = useApp();
  const [panel, setPanel] = useState<"skills" | "mcp">("skills");
  const [skills, setSkills] = useState<SkillInventoryRow[]>([]);
  const [mcp, setMcp] = useState<McpInventoryRow[]>([]);
  const [roots, setRoots] = useState<api.CanonicalRoots | null>(null);
  const [loading, setLoading] = useState(true);
  const [busyPath, setBusyPath] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const activeProject = projects[0] ?? null;

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

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const managed = useMemo(() => skills.filter((row) => row.status === "managed"), [skills]);
  const discovered = useMemo(
    () => skills.filter((row) => row.status !== "managed"),
    [skills],
  );

  const adopt = async (row: SkillInventoryRow, target: "user" | "project") => {
    if (row.system || row.read_only) return;
    setBusyPath(row.path);
    try {
      if (target === "user") {
        await api.adoptSkillToUser(row.path);
        toast.success(`Imported ${row.name} to User ~/.agents/skills`);
      } else {
        if (!activeProject) {
          toast.error("Link a project workspace first");
          return;
        }
        await api.adoptSkillToProject(row.path, activeProject.path);
        toast.success(`Imported ${row.name} to Project .agents/skills`);
      }
      await refresh();
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, "Import failed"));
    } finally {
      setBusyPath(null);
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
          <button
            onClick={() => void refresh()}
            className="inline-flex items-center gap-1.5 rounded-md border border-border px-2.5 py-1.5 text-sm text-secondary hover:bg-surface-hover"
          >
            <RefreshCw className="h-3.5 w-3.5" />
            Refresh
          </button>
        </div>
        <div className="grid gap-2 text-sm text-tertiary md:grid-cols-2">
          <div className="rounded-md border border-border-subtle bg-surface px-3 py-2">
            <div className="text-[11px] font-semibold uppercase tracking-wide text-muted">User</div>
            <div className="truncate text-secondary">{roots?.user_skills ?? "…"}</div>
          </div>
          <div className="rounded-md border border-border-subtle bg-surface px-3 py-2">
            <div className="text-[11px] font-semibold uppercase tracking-wide text-muted">Project</div>
            <div className="truncate text-secondary">
              {activeProject ? `${activeProject.path}\\.agents\\skills` : "No project linked"}
            </div>
          </div>
        </div>
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
            <h2 className="mb-2 text-sm font-semibold text-primary">Managed Skills</h2>
            {managed.length === 0 ? (
              <p className="text-sm text-muted">Nothing in ~/.agents/skills yet.</p>
            ) : (
              <div className="flex flex-col gap-2">
                {managed.map((row) => (
                  <SkillRow key={row.path} row={row} />
                ))}
              </div>
            )}
          </section>
          <section>
            <h2 className="mb-2 text-sm font-semibold text-primary">Discovered Elsewhere</h2>
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
                  <span className="text-[11px] uppercase text-muted">{row.transport}</span>
                </div>
                <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-xs text-tertiary">
                  {row.sources
                    .filter((source) => source.configured)
                    .map((source) => (
                      <span key={source.harness}>
                        {source.display_name}{" "}
                        {source.source_enabled === false ? "Disabled" : "Enabled"}
                      </span>
                    ))}
                </div>
                {expanded === row.name ? (
                  <div className="mt-2 space-y-1 text-xs text-secondary">
                    {row.command ? <div>Command: {row.command} {row.args.join(" ")}</div> : null}
                    {row.url ? <div>URL: {row.url}</div> : null}
                    {row.sources
                      .filter((source) => source.configured)
                      .map((source) => (
                        <div key={`${source.harness}-path`}>{source.source_path}</div>
                      ))}
                    <div className="text-muted">Read-only inventory. No enable/disable.</div>
                  </div>
                ) : null}
              </button>
            ))
          )}
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
}: {
  row: SkillInventoryRow;
  busy?: boolean;
  onAdoptUser?: () => void;
  onAdoptProject?: () => void;
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
        {onAdoptUser && !row.system ? (
          <div className="flex shrink-0 gap-1">
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
          </div>
        ) : null}
      </div>
    </div>
  );
}
