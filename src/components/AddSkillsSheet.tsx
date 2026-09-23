import { useCallback, useEffect, useMemo, useState, type MouseEvent } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { CircleSlash, Loader2, Search, X } from "lucide-react";
import { cn } from "../utils";
import * as api from "../lib/tauri";
import type { ManagedSkill } from "../lib/tauri";
import { getErrorMessage } from "../lib/error";
import {
  classifySkill,
  canInstallToProject,
  type PickerContext,
} from "../lib/skillPickerStatus";
import {
  getTagActiveColor,
  getTagColor,
  UNTAGGED_FILTER,
} from "../lib/skillTags";
import { SkillPickerRow } from "./SkillPickerRow";

const SOURCE_PRIORITY = ["local", "import", "git", "skillssh"];

export interface ProjectSheetTarget {
  kind: "project";
  projectId: string;
  projectName: string;
  /** dir/relative_path names already used under `<repo>/.agents/skills` */
  projectSkillDirNames: string[];
  /** managed skill ids already installed in the canonical project root */
  projectCenterSkillIds: string[];
}

interface Props {
  open: boolean;
  onClose: () => void;
  target: ProjectSheetTarget;
  managedSkills: ManagedSkill[];
  /** Called after one or more skills successfully installed. */
  onInstalled: () => Promise<void> | void;
}

export function AddSkillsSheet(props: Props) {
  if (!props.open) return null;
  return createPortal(<AddSkillsSheetBody {...props} />, document.body);
}

function AddSkillsSheetBody({ onClose, target, managedSkills, onInstalled }: Props) {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [tagFilters, setTagFilters] = useState<Set<string>>(new Set());
  const [sourceFilters, setSourceFilters] = useState<Set<string>>(new Set());
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [anchorId, setAnchorId] = useState<string | null>(null);
  const [installing, setInstalling] = useState(false);

  const [dirNameMap, setDirNameMap] = useState<Record<string, string>>({});
  const [dirNameMapError, setDirNameMapError] = useState(false);
  const [dirNameMapLoading, setDirNameMapLoading] = useState(true);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !installing) onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [installing, onClose]);

  // For project mode: precompute slugified dir names for managed skills
  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      const names = managedSkills.map((s) => s.name);
      if (names.length === 0) {
        if (!cancelled) {
          setDirNameMap({});
          setDirNameMapError(false);
          setDirNameMapLoading(false);
        }
        return;
      }
      setDirNameMapLoading(true);
      try {
        const slugified = await api.slugifySkillNames(names);
        if (cancelled) return;
        const map: Record<string, string> = {};
        managedSkills.forEach((s, i) => {
          map[s.id] = slugified[i];
        });
        setDirNameMap(map);
        setDirNameMapError(false);
      } catch {
        if (cancelled) return;
        setDirNameMap({});
        setDirNameMapError(true);
      } finally {
        if (!cancelled) setDirNameMapLoading(false);
      }
    };
    load();
    return () => {
      cancelled = true;
    };
  }, [managedSkills]);

  const ctx: PickerContext = useMemo(() => {
    return {
      kind: "project",
      projectSkillDirNames: target.projectSkillDirNames,
      projectCenterSkillIds: target.projectCenterSkillIds,
      dirNameMap,
      dirNameMapError,
    };
  }, [target, dirNameMap, dirNameMapError]);

  const allTags = useMemo(() => {
    const tags = new Set<string>();
    for (const skill of managedSkills) {
      for (const tag of skill.tags) {
        if (tag.trim()) tags.add(tag);
      }
    }
    return Array.from(tags).sort((a, b) => a.localeCompare(b));
  }, [managedSkills]);

  const sourceTypes = useMemo(() => {
    const present = new Set(managedSkills.map((s) => s.source_type).filter(Boolean));
    return [
      ...SOURCE_PRIORITY.filter((s) => present.has(s)),
      ...Array.from(present).filter((s) => !SOURCE_PRIORITY.includes(s)).sort(),
    ];
  }, [managedSkills]);

  const sourceLabel = useCallback(
    (source: string) => {
      if (SOURCE_PRIORITY.includes(source)) {
        return t(`mySkills.sourceFilter.${source}`);
      }
      return source;
    },
    [t],
  );

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    const hasUntagged = tagFilters.has(UNTAGGED_FILTER);
    const tagSelected = tagFilters.size > 0;
    return managedSkills.filter((skill) => {
      if (q) {
        const matches =
          skill.name.toLowerCase().includes(q) ||
          (skill.description || "").toLowerCase().includes(q);
        if (!matches) return false;
      }
      if (sourceFilters.size > 0 && !sourceFilters.has(skill.source_type)) return false;
      if (tagSelected) {
        const matchUntagged = hasUntagged && skill.tags.length === 0;
        const matchTag = skill.tags.some((tag) => tagFilters.has(tag));
        if (!matchUntagged && !matchTag) return false;
      }
      return true;
    });
  }, [managedSkills, search, sourceFilters, tagFilters]);

  // Sort: available first, then installed/conflict/unavailable (greyed out at bottom)
  const ordered = useMemo(() => {
    const statusOrder = { available: 0, conflict: 1, installed: 2, unavailable: 3 } as const;
    return [...filtered].sort((a, b) => {
      const sa = classifySkill(a, ctx);
      const sb = classifySkill(b, ctx);
      if (sa !== sb) return statusOrder[sa] - statusOrder[sb];
      return a.name.localeCompare(b.name);
    });
  }, [filtered, ctx]);

  // IDs of skills the user can add (status "available") in the current filtered view.
  const availableIds = useMemo(
    () => ordered.filter((s) => classifySkill(s, ctx) === "available").map((s) => s.id),
    [ordered, ctx],
  );
  const allAvailableSelected =
    availableIds.length > 0 && availableIds.every((id) => selectedIds.has(id));

  const toggleSelectAll = () => {
    if (availableIds.length === 0) return;
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (allAvailableSelected) {
        for (const id of availableIds) next.delete(id);
      } else {
        for (const id of availableIds) next.add(id);
      }
      return next;
    });
  };

  const skillsHaveUntagged = useMemo(
    () => managedSkills.some((s) => s.tags.length === 0),
    [managedSkills],
  );

  const toggleSelect = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  // Normal click toggles one row and moves the shift-click anchor there.
  const handleRowClick = (index: number) => (e: MouseEvent<HTMLDivElement>) => {
    const skill = ordered[index];
    if (e.shiftKey && anchorId) {
      const anchorIndex = ordered.findIndex((s) => s.id === anchorId);
      if (anchorIndex !== -1) {
        const [lo, hi] = anchorIndex <= index ? [anchorIndex, index] : [index, anchorIndex];
        const rangeIds = ordered
          .slice(lo, hi + 1)
          .map((s) => s.id)
          .filter((id) => availableIds.includes(id));
        if (rangeIds.length > 0) {
          const alreadyAllSelected = rangeIds.every((id) => selectedIds.has(id));
          setSelectedIds((prev) => {
            const next = new Set(prev);
            for (const id of rangeIds) {
              if (alreadyAllSelected) next.delete(id);
              else next.add(id);
            }
            return next;
          });
          return;
        }
      }
    }
    toggleSelect(skill.id);
    setAnchorId(skill.id);
  };

  const toggleSourceFilter = (source: string) => {
    setSourceFilters((prev) => {
      const next = new Set(prev);
      if (next.has(source)) next.delete(source);
      else next.add(source);
      return next;
    });
  };

  const toggleTagFilter = (tag: string) => {
    setTagFilters((prev) => {
      const next = new Set(prev);
      if (next.has(tag)) next.delete(tag);
      else next.add(tag);
      return next;
    });
  };

  const selectableSelected = useMemo(
    () => Array.from(selectedIds).filter((id) => {
      const skill = managedSkills.find((s) => s.id === id);
      if (!skill) return false;
      return classifySkill(skill, ctx) === "available";
    }),
    [selectedIds, managedSkills, ctx],
  );

  const projectNamesReady = dirNameMapError || !dirNameMapLoading;

  const ctaLabel = (() => {
    const count = selectableSelected.length;
    return count === 0
      ? t("addFromLibrary.ctaEmpty", { count: 0 })
      : t("addFromLibrary.cta", { count });
  })();

  const handleInstall = async () => {
    if (selectableSelected.length === 0) return;
    setInstalling(true);
    let ok = 0;
    let failed = 0;
    const failures: string[] = [];
    try {
      if (!projectNamesReady) return;
      for (const id of selectableSelected) {
        try {
          const skill = managedSkills.find((s) => s.id === id);
          if (!skill) continue;
          if (!canInstallToProject(skill, ctx)) continue;
          await api.exportSkillToProject(id, target.projectId);
          ok++;
        } catch (e) {
          failed++;
          failures.push(getErrorMessage(e, t("common.error")));
        }
      }
      if (ok > 0) {
        toast.success(t("addFromLibrary.toastInstalled", { count: ok }));
        setSelectedIds(new Set());
      }
      if (failed > 0) {
        // Surface why. A refusal carries the path it protected and what to do
        // about it (#363); collapsing that to a bare count leaves the user with
        // no idea which skill failed or how to resolve it.
        const detail = failures[0];
        toast.error(
          failed === 1 && detail
            ? detail
            : [t("addFromLibrary.toastFailed", { count: failed }), detail]
                .filter(Boolean)
                .join(" — "),
        );
      }
      await onInstalled();
      if (failed === 0) onClose();
    } catch (e) {
      toast.error(getErrorMessage(e, t("common.error")));
    } finally {
      setInstalling(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50">
      <div
        className="absolute inset-0 bg-black/40 backdrop-blur-[1px]"
        onClick={() => !installing && onClose()}
      />
      <div className="absolute right-0 top-0 flex h-full w-full max-w-[480px] flex-col overflow-hidden border-l border-border-subtle bg-bg-secondary shadow-2xl">
        <div className="flex shrink-0 items-start justify-between gap-3 border-b border-border-subtle px-5 py-4">
          <div className="min-w-0 flex-1">
            <h2 className="text-[14px] font-semibold text-primary">
              {t("addFromLibrary.title")}
            </h2>
            <div className="mt-2 text-[12px] text-muted">{target.projectName}</div>
          </div>
          <button
            onClick={onClose}
            disabled={installing}
            className="shrink-0 rounded-md p-1.5 text-muted transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="shrink-0 border-b border-border-subtle px-5 py-3">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t("addFromLibrary.searchPlaceholder")}
              className="app-input w-full pl-9"
              autoFocus
            />
          </div>

          {(allTags.length > 0 || skillsHaveUntagged) && (
            <div className="mt-2 flex flex-wrap items-center gap-1.5">
              <span className="text-[12px] text-muted">{t("mySkills.tags.filter")}</span>
              <button
                onClick={() => setTagFilters(new Set())}
                className={cn(
                  "rounded-full px-2.5 py-0.5 text-[12px] font-medium transition-colors",
                  tagFilters.size === 0
                    ? "bg-accent text-white dark:bg-accent dark:text-white"
                    : "bg-surface-hover text-muted hover:text-secondary",
                )}
              >
                {t("mySkills.tags.allTags")}
              </button>
              {skillsHaveUntagged && (
                <button
                  onClick={() => toggleTagFilter(UNTAGGED_FILTER)}
                  className={cn(
                    "inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-[12px] font-medium transition-colors",
                    tagFilters.has(UNTAGGED_FILTER)
                      ? "bg-surface-active text-primary"
                      : "border border-dashed border-border text-muted hover:text-secondary",
                  )}
                  title={t("mySkills.tags.untagged")}
                >
                  <CircleSlash className="h-3 w-3" />
                  {t("mySkills.tags.untagged")}
                </button>
              )}
              {allTags.map((tag) => {
                const active = tagFilters.has(tag);
                return (
                  <button
                    key={tag}
                    onClick={() => toggleTagFilter(tag)}
                    className={cn(
                      "rounded-full px-2.5 py-0.5 text-[12px] font-medium transition-colors",
                      active ? getTagActiveColor(tag, allTags) : getTagColor(tag, allTags),
                    )}
                  >
                    {tag}
                  </button>
                );
              })}
            </div>
          )}

          {sourceTypes.length > 1 && (
            <div className="mt-2 flex flex-wrap items-center gap-1.5">
              <span className="text-[12px] text-muted">{t("mySkills.sourceType")}</span>
              {sourceTypes.map((source) => {
                const active = sourceFilters.has(source);
                return (
                  <button
                    key={source}
                    onClick={() => toggleSourceFilter(source)}
                    className={cn(
                      "rounded-full px-2.5 py-0.5 text-[12px] font-medium transition-colors",
                      active
                        ? "bg-accent text-white dark:bg-accent dark:text-white"
                        : "bg-surface-hover text-muted hover:text-secondary",
                    )}
                  >
                    {sourceLabel(source)}
                  </button>
                );
              })}
            </div>
          )}
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto scrollbar-hide">
          {ordered.length === 0 ? (
            <div className="px-5 py-12 text-center text-[13px] text-muted">
              {managedSkills.length === 0
                ? t("addFromLibrary.emptyLibrary")
                : t("addFromLibrary.emptyMatch")}
            </div>
          ) : (
            <div className="divide-y divide-border-subtle">
              {ordered.map((skill, index) => {
                const status = classifySkill(skill, ctx);
                return (
                  <SkillPickerRow
                    key={skill.id}
                    skill={skill}
                    status={status}
                    allTags={allTags}
                    sourceLabel={sourceLabel(skill.source_type)}
                    selected={selectedIds.has(skill.id)}
                    onToggle={handleRowClick(index)}
                  />
                );
              })}
            </div>
          )}
        </div>

        <div className="shrink-0 border-t border-border-subtle bg-bg-secondary px-5 py-3">
          <div className="mb-2 flex items-center justify-between gap-2">
            <span className="truncate text-[12px] text-muted">
              {selectableSelected.length > 0
                ? t("addFromLibrary.selectedCount", { count: selectableSelected.length })
                : t("addFromLibrary.shiftHint")}
            </span>
            <button
              type="button"
              onClick={toggleSelectAll}
              disabled={availableIds.length === 0}
              className="shrink-0 rounded-md px-2.5 py-1 text-[12px] font-medium text-muted transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
            >
              {allAvailableSelected
                ? t("addFromLibrary.deselectAllSkills")
                : t("addFromLibrary.selectAllSkills")}
            </button>
          </div>
          <button
            onClick={handleInstall}
            disabled={
              installing ||
              !projectNamesReady ||
              selectableSelected.length === 0
            }
            className="inline-flex w-full items-center justify-center gap-1.5 rounded-md bg-accent px-3 py-2.5 text-[13px] font-medium text-white transition-colors hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-50"
          >
            {installing ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
            {ctaLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
