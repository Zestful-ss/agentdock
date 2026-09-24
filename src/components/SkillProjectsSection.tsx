import { useEffect, useMemo, useState } from "react";
import { AlertTriangle, CheckCircle2, FolderOpen, Loader2, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import * as api from "../lib/tauri";
import type { ManagedSkill, Project } from "../lib/tauri";
import { cn } from "../utils";
import { getErrorMessage } from "../lib/error";

interface Props {
  skill: ManagedSkill;
  projects: Project[];
  onChanged?: () => void;
}

type RowState = "loading" | "installed" | "available" | "conflict" | "error";

interface RowData {
  state: RowState;
  installedPath?: string;
  dirNames: string[];
  dirName?: string;
  error?: string;
}

export function SkillProjectsSection({ skill, projects, onChanged }: Props) {
  const { t } = useTranslation();
  const [rows, setRows] = useState<Record<string, RowData>>({});
  const [pendingKey, setPendingKey] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setRows((prev) => {
      const next: Record<string, RowData> = {};
      for (const p of projects) {
        next[p.id] = prev[p.id] ?? {
          state: "loading",
          dirNames: [],
        };
      }
      return next;
    });

    const loadAll = async () => {
      const results = await Promise.all(
        projects.map(async (p) => {
          try {
            const [projectSkills, dirNames] = await Promise.all([
              api.getProjectSkills(p.id),
              api.slugifySkillNames([skill.name]),
            ]);
            const dirName = dirNames[0]?.toLowerCase();
            const dirNamesLower = projectSkills.map((s) => s.relative_path.toLowerCase());
            const installed = projectSkills.find((s) => s.center_skill_id === skill.id);
            let state: RowState = "available";
            if (installed) {
              state = "installed";
            } else if (dirName && dirNamesLower.includes(dirName)) {
              state = "conflict";
            }
            return [p.id, {
              state,
              installedPath: installed?.relative_path,
              dirNames: dirNamesLower,
              dirName,
            }] as const;
          } catch (e) {
            return [p.id, {
              state: "error" as const,
              dirNames: [],
              error: getErrorMessage(e, ""),
            }] as const;
          }
        }),
      );
      if (cancelled) return;
      setRows(Object.fromEntries(results));
    };
    void loadAll();
    return () => {
      cancelled = true;
    };
  }, [projects, skill.id, skill.name]);

  const installedCount = useMemo(
    () => Object.values(rows).filter((r) => r.state === "installed").length,
    [rows],
  );

  const handleAdd = async (project: Project) => {
    const row = rows[project.id];
    if (!row || row.state !== "available") return;
    const key = project.id;
    setPendingKey(key);
    try {
      await api.copySkillToProject(skill.id, project.id);
      toast.success(
        t("addFromLibrary.toastAddedToProject", {
          skill: skill.name,
          project: project.name,
        }),
      );
      setRows((prev) => ({
        ...prev,
        [project.id]: {
          ...row,
          state: "installed",
          installedPath: row.dirName ?? skill.name,
        },
      }));
      onChanged?.();
    } catch (e) {
      toast.error(getErrorMessage(e, t("common.error")));
    } finally {
      setPendingKey(null);
    }
  };

  const handleRemove = async (project: Project) => {
    const row = rows[project.id];
    if (!row || row.state !== "installed" || !row.installedPath) return;
    const key = project.id;
    setPendingKey(key);
    try {
      await api.deleteProjectSkill(project.id, row.installedPath);
      toast.success(
        t("addFromLibrary.toastRemovedFromProject", {
          skill: skill.name,
          project: project.name,
        }),
      );
      const removedDirName = row.installedPath.toLowerCase();
      setRows((prev) => ({
        ...prev,
        [project.id]: {
          ...row,
          state: "available",
          installedPath: undefined,
          dirNames: row.dirNames.filter((dirName) => dirName !== removedDirName),
        },
      }));
      onChanged?.();
    } catch (e) {
      toast.error(getErrorMessage(e, t("common.error")));
    } finally {
      setPendingKey(null);
    }
  };

  if (projects.length === 0) return null;

  const visibleProjects = expanded ? projects : projects.slice(0, 4);

  return (
    <div className="mb-4 rounded-xl border border-border-subtle">
      <div className="flex items-center justify-between gap-2 border-b border-border-subtle px-6 py-2.5 text-[13px]">
        <div className="flex min-w-0 items-center gap-2">
          <span className="font-medium text-secondary">
            {t("addFromLibrary.projectsTitle")}
          </span>
          <span className="rounded-full border border-border-subtle bg-surface px-2 py-0.5 text-[12px] text-muted">
            {t("addFromLibrary.projectsSummary", {
              installed: installedCount,
              total: projects.length,
            })}
          </span>
        </div>
        {projects.length > 4 && (
          <button
            type="button"
            onClick={() => setExpanded((prev) => !prev)}
            className="text-[12px] text-muted hover:text-secondary"
          >
            {expanded ? t("common.collapse") : t("common.expandAll")}
          </button>
        )}
      </div>
      <div className="grid grid-cols-1 gap-1.5 px-3 py-3 md:grid-cols-2">
        {visibleProjects.map((project) => {
          const row = rows[project.id];
          const agentPending = pendingKey === project.id;
          const label =
            row?.state === "installed"
              ? t("addFromLibrary.installedShort")
              : row?.state === "conflict"
                ? t("addFromLibrary.status.conflict")
                : t("addFromLibrary.add");
          const title =
            row?.state === "conflict"
              ? t("addFromLibrary.tooltip.conflict")
              : row?.state === "installed"
                ? t("addFromLibrary.tooltip.remove")
                : project.name;
          return (
            <div
              key={project.id}
              className={cn(
                "rounded-md border border-border-subtle bg-background px-3 py-2 text-[12.5px]",
                row?.state === "installed" && "border-emerald-500/30 bg-emerald-500/5",
              )}
            >
              <div className="flex min-w-0 flex-col gap-1.5">
                <div className="flex min-w-0 items-center gap-2">
                  <FolderOpen className="h-3.5 w-3.5 shrink-0 text-muted" />
                  <span className="min-w-0 flex-1 truncate font-medium text-secondary" title={project.name}>
                    {project.name}
                  </span>
                  {!row || row.state === "loading" ? (
                    <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-faint" />
                  ) : row.state === "error" ? (
                    <span
                      className="shrink-0 text-rose-500"
                      title={row.error || t("common.error")}
                    >
                      {t("common.error")}
                    </span>
                  ) : (
                    <button
                      type="button"
                      title={title}
                      onClick={() => {
                        if (row.state === "installed") {
                          void handleRemove(project);
                        } else {
                          void handleAdd(project);
                        }
                      }}
                      disabled={(row.state !== "available" && row.state !== "installed") || agentPending}
                      className={cn(
                        "inline-flex h-7 shrink-0 items-center gap-1 rounded-md px-1.5 text-[12px] font-semibold transition-colors disabled:cursor-default",
                        row.state === "available" && "text-accent-light hover:bg-accent-bg",
                        row.state === "installed" && "bg-emerald-500/10 text-emerald-600 hover:bg-emerald-500/15 dark:text-emerald-400",
                        row.state === "conflict" && "bg-rose-500/10 text-rose-600 dark:text-rose-400",
                      )}
                    >
                      {agentPending ? (
                        <Loader2 className="h-3 w-3 animate-spin" />
                      ) : row.state === "installed" ? (
                        <CheckCircle2 className="h-3 w-3" />
                      ) : row.state === "conflict" ? (
                        <AlertTriangle className="h-3 w-3" />
                      ) : (
                        <Plus className="h-3 w-3" />
                      )}
                      <span>{label}</span>
                    </button>
                  )}
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
