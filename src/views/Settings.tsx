import { useState, useEffect, useCallback, useMemo } from "react";
import {
  Folder,
  FolderOpen,
  RefreshCw,
  Link as LinkIcon,
  Settings2,
  Github,
  Globe,
  Loader2,
  ExternalLink,
  Sun,
  Moon,
  Monitor,
  AlertTriangle,
  BookOpen,
  Bug,
  FileArchive,
  Type,
  Pencil,
  RotateCcw,
  Plus,
  Trash2,
  X,
  Check,
  ChevronDown,
  ChevronRight,
  GripVertical,
} from "lucide-react";
import {
  DndContext,
  closestCenter,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  useSortable,
  arrayMove,
  rectSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { openUrl } from "@tauri-apps/plugin-opener";
import { writeText as clipboardWriteText } from "@tauri-apps/plugin-clipboard-manager";
import { open as dialogOpen, confirm as dialogConfirm } from "@tauri-apps/plugin-dialog";
import { cn } from "../utils";
import { useApp } from "../context/AppContext";
import { useThemeContext } from "../context/ThemeContext";
import { AgentIcon } from "../components/AgentIcon";
import { ToggleSwitch } from "../components/ToggleSwitch";
import * as api from "../lib/tauri";
import { applyTextSize } from "../lib/textScale";
import { getErrorMessage } from "../lib/error";
import type { Theme } from "../hooks/useTheme";

function compactHomePath(path: string) {
  return path
    .replace(/\/Users\/[^/]+/, "~")
    .replace(/\/home\/[^/]+/, "~")
    .replace(/^[A-Za-z]:\\Users\\[^\\]+/, "~");
}

interface SortableAgentCardProps {
  agentKey: string;
  dragLabel: string;
  children: (dragHandle: React.ReactNode) => React.ReactNode;
}

function SortableAgentCard({ agentKey, dragLabel, children }: SortableAgentCardProps) {
  const {
    attributes,
    listeners,
    setNodeRef,
    setActivatorNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: agentKey });

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.5 : undefined,
  };

  const handle = (
    <button
      type="button"
      ref={setActivatorNodeRef}
      {...listeners}
      onClick={(e) => e.stopPropagation()}
      className="mt-0.5 flex shrink-0 cursor-grab items-center justify-center rounded text-faint outline-none transition-colors hover:text-muted active:cursor-grabbing"
      title={dragLabel}
      aria-label={dragLabel}
    >
      <GripVertical className="h-3.5 w-3.5" />
    </button>
  );

  return (
    <div ref={setNodeRef} style={style} {...attributes} className="h-full">
      {children(handle)}
    </div>
  );
}

interface AgentGroupDndProps {
  items: api.ToolInfo[];
  sensors: ReturnType<typeof useSensors>;
  dragLabel: string;
  onDragEnd: (event: DragEndEvent, groupKeys: string[]) => void;
  renderAgentCard: (agent: api.ToolInfo, dragHandle?: React.ReactNode) => React.ReactNode;
}

function AgentGroupDnd({ items, sensors, dragLabel, onDragEnd, renderAgentCard }: AgentGroupDndProps) {
  const groupKeys = items.map((t) => t.key);
  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragEnd={(e) => onDragEnd(e, groupKeys)}
    >
      <SortableContext items={groupKeys} strategy={rectSortingStrategy}>
        <div className="grid grid-cols-1 gap-1.5 md:grid-cols-2 xl:grid-cols-3">
          {items.map((agent) => (
            <SortableAgentCard key={agent.key} agentKey={agent.key} dragLabel={dragLabel}>
              {(handle) => renderAgentCard(agent, handle)}
            </SortableAgentCard>
          ))}
        </div>
      </SortableContext>
    </DndContext>
  );
}

export function Settings() {
  const { t, i18n } = useTranslation();
  const { tools, refreshTools, openHelp } = useApp();
  const [togglingTools, setTogglingTools] = useState<Set<string>>(new Set());
  const { theme, setTheme } = useThemeContext();
  const [refreshing, setRefreshing] = useState(false);
  const [openingRepo, setOpeningRepo] = useState(false);
  const [openingGithub, setOpeningGithub] = useState(false);
  const [reportingIssue, setReportingIssue] = useState(false);
  const [exportingLogs, setExportingLogs] = useState(false);
  const [lastPanic, setLastPanic] = useState<api.PanicInfo | null>(null);
  const [repoWarnings, setRepoWarnings] = useState<string[]>([]);
  const [centralRepoPath, setCentralRepoPath] = useState("");
  const [centralRepoPathOverride, setCentralRepoPathOverride] = useState<string | null>(null);
  const [editingCentralRepoPath, setEditingCentralRepoPath] = useState(false);
  const [centralRepoPathInput, setCentralRepoPathInput] = useState("");
  const [savingCentralRepoPath, setSavingCentralRepoPath] = useState(false);
  const [proxyInput, setProxyInput] = useState("");
  const [proxySaving, setProxySaving] = useState(false);
  const [textSize, setTextSize] = useState("default");
  // Agent path editing
  const [editingPathKey, setEditingPathKey] = useState<string | null>(null);
  const [editingPathValue, setEditingPathValue] = useState("");
  // Project path editing (custom agents only)
  const [editingProjectPathKey, setEditingProjectPathKey] = useState<string | null>(null);
  const [editingProjectPathValue, setEditingProjectPathValue] = useState("");
  // Custom agent dialog
  const [showAddCustom, setShowAddCustom] = useState(false);
  const [customName, setCustomName] = useState("");
  const [customPath, setCustomPath] = useState("");
  const [customProjectPath, setCustomProjectPath] = useState("");
  const [addingCustom, setAddingCustom] = useState(false);
  const [showMoreAgents, setShowMoreAgents] = useState(false);
  const [readOnlyPaths, setReadOnlyPaths] = useState<string[]>([]);
  const [readOnlyPathInput, setReadOnlyPathInput] = useState("");
  const [savingReadOnlyPaths, setSavingReadOnlyPaths] = useState(false);

  const GITHUB_URL = "https://github.com/Zestful-ss/agentdock";
  const WEBSITE_URL = "https://skillsmanager.dev";

  const startEditPath = useCallback((key: string, currentPath: string) => {
    setEditingPathKey(key);
    setEditingPathValue(currentPath);
  }, []);

  const handleSavePath = async () => {
    if (!editingPathKey || !editingPathValue.trim()) return;
    try {
      await api.setCustomToolPath(editingPathKey, editingPathValue.trim());
      await refreshTools();
      toast.success(t("settings.pathSaved"));
    } catch (e) {
      toast.error(String(e));
    } finally {
      setEditingPathKey(null);
    }
  };

  const startEditProjectPath = useCallback((key: string, currentPath: string | null) => {
    setEditingProjectPathKey(key);
    setEditingProjectPathValue(currentPath ?? "");
  }, []);

  const handleSaveProjectPath = async () => {
    if (!editingProjectPathKey) return;
    const trimmed = editingProjectPathValue.trim();
    try {
      await api.setCustomToolProjectPath(editingProjectPathKey, trimmed || null);
      await refreshTools();
      toast.success(t("settings.pathSaved"));
      setEditingProjectPathKey(null);
    } catch (e) {
      toast.error(String(e));
    }
  };

  const handleResetProjectPath = async (key: string) => {
    try {
      await api.resetCustomToolProjectPath(key);
      await refreshTools();
      toast.success(t("settings.projectPathReset"));
    } catch {
      toast.error(t("common.error"));
    }
  };

  const handleResetPath = async (key: string) => {
    try {
      await api.resetCustomToolPath(key);
      await refreshTools();
      toast.success(t("settings.pathReset"));
    } catch {
      toast.error(t("common.error"));
    }
  };

  const handleBrowsePath = async (setter: (v: string) => void) => {
    const selected = await dialogOpen({ directory: true, multiple: false });
    if (selected && typeof selected === "string") {
      setter(selected);
    }
  };

  const handleBrowseReadOnlyPath = async () => {
    await handleBrowsePath(setReadOnlyPathInput);
  };

  const handleAddReadOnlyPath = async () => {
    const path = readOnlyPathInput.trim();
    if (!path) return;
    setSavingReadOnlyPaths(true);
    try {
      const next = await api.setCustomReadOnlyPaths([...readOnlyPaths, path]);
      setReadOnlyPaths(next);
      setReadOnlyPathInput("");
      toast.success(t("settings.readOnlyPathAdded"));
    } catch (error) {
      toast.error(String(error));
    } finally {
      setSavingReadOnlyPaths(false);
    }
  };

  const handleRemoveReadOnlyPath = async (path: string) => {
    setSavingReadOnlyPaths(true);
    try {
      const next = await api.setCustomReadOnlyPaths(readOnlyPaths.filter((item) => item !== path));
      setReadOnlyPaths(next);
    } catch (error) {
      toast.error(String(error));
    } finally {
      setSavingReadOnlyPaths(false);
    }
  };

  const generateCustomAgentKey = useCallback(
    (name: string) => {
      const base = name
        .trim()
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "_")
        .replace(/^_+|_+$/g, "");
      const seed = base || "agent";
      const existingKeys = new Set(tools.map((tool) => tool.key));
      if (!existingKeys.has(seed)) return seed;
      let n = 2;
      while (existingKeys.has(`${seed}_${n}`)) n += 1;
      return `${seed}_${n}`;
    },
    [tools]
  );

  const handleAddCustomAgent = async () => {
    const trimName = customName.trim();
    const trimPath = customPath.trim();
    const trimProjectPath = customProjectPath.trim();
    if (!trimName || !trimPath) return;
    const trimKey = generateCustomAgentKey(trimName);
    setAddingCustom(true);
    try {
      await api.addCustomTool(trimKey, trimName, trimPath, trimProjectPath || undefined);
      await refreshTools();
      toast.success(t("settings.customAgentAdded"));
      setShowAddCustom(false);
      setCustomName("");
      setCustomPath("");
      setCustomProjectPath("");
    } catch (e) {
      toast.error(String(e));
    } finally {
      setAddingCustom(false);
    }
  };

  const handleRemoveCustomAgent = async (key: string, name: string) => {
    const shouldRemove = await dialogConfirm(t("settings.removeCustomAgentConfirm", { name }));
    if (!shouldRemove) return;
    try {
      await api.removeCustomTool(key);
      await refreshTools();
      toast.success(t("settings.customAgentRemoved"));
    } catch {
      toast.error(t("common.error"));
    }
  };

  useEffect(() => {
    api.checkLastPanic().then(setLastPanic).catch(() => {});
    api.getCentralRepoWarnings().then(setRepoWarnings).catch(() => {});
  }, []);

  useEffect(() => {
    api.getSettings("proxy_url").then((v) => { setProxyInput(v ?? ""); });
    api.getSettings("text_size").then((v) => { if (v) { setTextSize(v); applyTextSize(v); } });
    api.getCentralRepoPath().then((path) => {
      setCentralRepoPath(path);
      setCentralRepoPathInput(path);
    }).catch(() => {});
    api.getCentralRepoPathOverride().then(setCentralRepoPathOverride).catch(() => {});
    api.getCustomReadOnlyPaths().then(setReadOnlyPaths).catch(() => {});
  }, []);

  const handleRefresh = async () => {
    setRefreshing(true);
    await refreshTools();
    setRefreshing(false);
    toast.success(t("common.success"));
  };

  const handleToggleTool = async (key: string, enabled: boolean) => {
    setTogglingTools((prev) => new Set(prev).add(key));
    try {
      await api.setToolEnabled(key, enabled);
      await refreshTools();
    } catch {
      toast.error(t("common.error"));
    } finally {
      setTogglingTools((prev) => {
        const next = new Set(prev);
        next.delete(key);
        return next;
      });
    }
  };

  const handleToggleAllTools = async (enabled: boolean) => {
    try {
      await api.setAllToolsEnabled(enabled);
      await refreshTools();
      toast.success(t("common.success"));
    } catch {
      toast.error(t("common.error"));
    }
  };

  const handleLanguageChange = (lng: string) => {
    localStorage.setItem("language", lng);
    i18n.changeLanguage(lng);
    api.setSettings("language", lng);
  };

  const handleTextSizeChange = (size: string) => {
    setTextSize(size);
    applyTextSize(size);
    api.setSettings("text_size", size);
  };

  const handleOpenRepoInFinder = async () => {
    try {
      setOpeningRepo(true);
      await api.openCentralRepoFolder();
    } catch (error) {
      console.error("Failed to open central repository folder", error);
      toast.error(t("common.error"));
    } finally {
      setOpeningRepo(false);
    }
  };

  const handleStartEditCentralRepoPath = () => {
    setCentralRepoPathInput(centralRepoPathOverride ?? centralRepoPath);
    setEditingCentralRepoPath(true);
  };

  const handleSaveCentralRepoPath = async () => {
    const trimmed = centralRepoPathInput.trim();
    if (!trimmed) {
      toast.error(t("settings.repoPathEmpty"));
      return;
    }
    setSavingCentralRepoPath(true);
    try {
      const nextPath = await api.setCentralRepoPath(trimmed);
      setCentralRepoPath(nextPath);
      setCentralRepoPathOverride(nextPath);
      setEditingCentralRepoPath(false);
      toast.success(t("settings.repoPathSaved"));
      toast.info(t("settings.repoPathRestartNotice"));
    } catch (error) {
      toast.error(String(error));
    } finally {
      setSavingCentralRepoPath(false);
    }
  };

  const handleResetCentralRepoPath = async () => {
    setSavingCentralRepoPath(true);
    try {
      const nextPath = await api.setCentralRepoPath(null);
      setCentralRepoPath(nextPath);
      setCentralRepoPathOverride(null);
      setCentralRepoPathInput(nextPath);
      setEditingCentralRepoPath(false);
      toast.success(t("settings.repoPathReset"));
      toast.info(t("settings.repoPathRestartNotice"));
    } catch (error) {
      toast.error(String(error));
    } finally {
      setSavingCentralRepoPath(false);
    }
  };

  const handleOpenGithub = async () => {
    try {
      setOpeningGithub(true);
      await openUrl(GITHUB_URL);
    } catch (error) {
      console.error("Failed to open GitHub repository", error);
      toast.error(t("common.error"));
    } finally {
      setOpeningGithub(false);
    }
  };

  const handleExportLogs = async () => {
    setExportingLogs(true);
    try {
      const result = await api.exportLogsZip();
      toast.success(t("settings.exportLogsDone", { count: result.file_count }), {
        description: result.zip_path,
      });
    } catch (error) {
      console.error("Failed to export logs", error);
      toast.error(t("settings.exportLogsFailed"));
    } finally {
      setExportingLogs(false);
    }
  };

  const handleDismissPanic = async () => {
    try {
      await api.clearLastPanic();
    } catch (err) {
      console.warn("Failed to clear last_panic.log", err);
    }
    setLastPanic(null);
  };

  const handleReportIssue = async () => {
    setReportingIssue(true);
    try {
      const [info, logExcerpt, panicInfo] = await Promise.all([
        api.getDiagnosticInfo(),
        api.getRecentLogExcerpt().catch((err) => {
          console.warn("Failed to read log excerpt", err);
          return null;
        }),
        api.checkLastPanic().catch(() => null),
      ]);
      const enabledBuiltin = enabledTools
        .filter((tool) => !tool.is_custom)
        .map((tool) => tool.key);
      const enabledCustomCount = enabledTools.filter((tool) => tool.is_custom).length;
      const agentsLine = enabledBuiltin.length === 0 && enabledCustomCount === 0
        ? "(none)"
        : [
            enabledBuiltin.join(", "),
            enabledCustomCount > 0 ? `${enabledCustomCount} custom` : "",
          ].filter(Boolean).join(", ");
      const parts = [
        "**Diagnostics** (auto-collected by AgentDock)",
        "",
        `- App version: \`${info.app_version}\``,
        `- OS: \`${info.os} ${info.os_version} (${info.arch})\``,
        `- UI locale: \`${i18n.language}\``,
        `- Enabled agents: ${agentsLine}`,
        `- Central repo: \`${info.central_repo_path}\`${info.central_repo_path_overridden ? " (custom path)" : ""}`,
      ];
      if (panicInfo) {
        parts.push(
          "",
          `**Last panic** (${panicInfo.timestamp})`,
          "",
          "```",
          panicInfo.message,
          "```",
        );
      }
      if (logExcerpt) {
        parts.push(
          "",
          `**Recent log** (\`${logExcerpt.log_path}\`, ${logExcerpt.line_count} lines${logExcerpt.has_warnings ? ", includes warnings/errors" : ""})`,
          "",
          "```log",
          logExcerpt.excerpt,
          "```",
          "",
          `> ${t("settings.reportIssueExportHint")}`,
        );
      }
      const md = parts.join("\n");
      let copied = false;
      try {
        await clipboardWriteText(md);
        copied = true;
      } catch (err) {
        console.error("Clipboard write failed", err);
        try {
          await navigator.clipboard.writeText(md);
          copied = true;
        } catch (err2) {
          console.error("Browser clipboard fallback also failed", err2);
        }
      }
      try {
        await openUrl(`${GITHUB_URL}/issues/new?template=bug_report.md`);
      } catch (err) {
        console.error("Failed to open issue page", err);
      }
      if (copied) {
        toast.success(t("settings.diagnosticsCopied"));
        if (panicInfo) {
          try {
            await api.clearLastPanic();
          } catch (err) {
            console.warn("Failed to clear last_panic.log", err);
          }
          setLastPanic(null);
        }
      } else {
        toast.message(t("settings.diagnosticsCopyManual"), { description: md });
      }
    } catch (error) {
      console.error("Failed to prepare diagnostics", error);
      toast.error(t("common.error"));
    } finally {
      setReportingIssue(false);
    }
  };

  const handleSaveProxy = async () => {
    const trimmed = proxyInput.trim();
    if (trimmed && !/^(https?|socks5):\/\//i.test(trimmed)) {
      toast.error(t("settings.proxyUrlInvalid"));
      return;
    }
    setProxySaving(true);
    try {
      await api.setSettings("proxy_url", trimmed);
      toast.success(t("settings.proxyUrlSaved"));
    } catch {
      toast.error(t("common.error"));
    } finally {
      setProxySaving(false);
    }
  };

  // Compose the shared control classes from index.css rather than a parallel
  // set — bg-background keeps fields readable against the surface-colored panel.
  const fieldClass = "app-input bg-background";
  const actionButtonClass = "app-button-secondary gap-1.5";
  const segmentedButtonClass = "app-segmented-button flex items-center gap-1.5";

  const themeOptions: Array<{ value: Theme; label: string; icon: typeof Sun }> = [
    { value: "light", label: t("settings.themeLight"), icon: Sun },
    { value: "dark", label: t("settings.themeDark"), icon: Moon },
    { value: "system", label: t("settings.themeSystem"), icon: Monitor },
  ];
  const installedTools = useMemo(() => tools.filter((tool) => tool.installed), [tools]);
  const enabledTools = useMemo(
    () => installedTools.filter((tool) => tool.enabled),
    [installedTools]
  );
  const customTools = useMemo(() => tools.filter((tool) => tool.is_custom), [tools]);
  const builtInTools = useMemo(() => tools.filter((tool) => !tool.is_custom), [tools]);
  // Grouped by what is actually on this machine rather than by a hand-kept
  // "mainstream" list. A settings page reader cares about the agents they have,
  // and that list stays correct without anyone re-curating it as products rise
  // and fall. Both groups keep the backend's order, which is ranked by how
  // widely used each agent is (see DEFAULT_PRIORITY_ORDER) and overridden by
  // whatever the user has dragged.
  const detectedTools = useMemo(
    () => builtInTools.filter((tool) => tool.installed),
    [builtInTools]
  );
  const undetectedTools = useMemo(
    () => builtInTools.filter((tool) => !tool.installed),
    [builtInTools]
  );

  const dragSensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 5 } }));

  const handleAgentDragEnd = useCallback(
    async (event: DragEndEvent, groupKeys: string[]) => {
      const { active, over } = event;
      if (!over || active.id === over.id) return;
      const oldIdx = groupKeys.indexOf(String(active.id));
      const newIdx = groupKeys.indexOf(String(over.id));
      if (oldIdx < 0 || newIdx < 0) return;

      const newGroupKeys = arrayMove(groupKeys, oldIdx, newIdx);
      const fullOrder = tools.map((t) => t.key);
      const groupKeySet = new Set(groupKeys);
      let cursor = 0;
      const newFullOrder = fullOrder.map((k) =>
        groupKeySet.has(k) ? newGroupKeys[cursor++] : k
      );

      try {
        await api.setToolOrder(newFullOrder);
        await refreshTools();
      } catch (e) {
        toast.error(getErrorMessage(e, t("common.error")));
      }
    },
    [tools, refreshTools, t]
  );
  const displayedRepoPath = centralRepoPath
    ? compactHomePath(centralRepoPath)
    : t("common.loading");

  const renderAgentCard = (agent: typeof tools[number], dragHandle?: React.ReactNode) => (
    <div
      className={cn(
        "group relative flex h-full flex-col gap-1.5 rounded-xl border px-3.5 py-3 transition-colors",
        agent.installed && agent.enabled
          ? "border-border bg-surface"
          : agent.installed
            ? "border-border-subtle bg-surface"
            : "border-border-subtle bg-bg-secondary"
      )}
    >
      <div className="flex items-start gap-2.5">
        {dragHandle}
        <AgentIcon
          agentKey={agent.key}
          displayName={agent.display_name}
          className="mt-px h-6 w-6 shrink-0 rounded-md"
        />

        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h3
              className={cn(
                "truncate text-[14px] font-semibold",
                agent.installed ? "text-primary" : "text-muted"
              )}
            >
              {agent.display_name}
            </h3>
            {/* Enabled/disabled is carried by the switch; only "not installed" adds info. */}
            {!agent.installed && (
              <span className="shrink-0 rounded-full bg-surface-hover px-2 py-0.5 text-[10px] font-medium text-muted">
                {t("settings.notInstalled")}
              </span>
            )}
          </div>

          <div className="mt-0.5 flex flex-wrap items-center gap-1">
            {agent.is_custom && (
              <span className="rounded-full bg-sky-500/10 px-2 py-0.5 text-[10px] font-medium text-sky-700 dark:text-sky-300">
                {t("settings.customAgent")}
              </span>
            )}
            {agent.is_custom && agent.project_relative_skills_dir && (
              <span className="rounded-full bg-emerald-500/10 px-2 py-0.5 text-[10px] font-medium text-emerald-700 dark:text-emerald-300">
                {t("settings.projectAgentSupported")}
              </span>
            )}
            {agent.has_path_override && !agent.is_custom && (
              <span className="rounded-full bg-amber-500/10 px-2 py-0.5 text-[10px] font-medium text-amber-700 dark:text-amber-300">
                {t("settings.pathOverridden")}
              </span>
            )}
          </div>
        </div>

        {agent.is_custom && (
          <button
            onClick={() => handleRemoveCustomAgent(agent.key, agent.display_name)}
            className="mt-0.5 shrink-0 p-0.5 text-muted opacity-0 outline-none transition-opacity hover:text-red-500 group-hover:opacity-100"
            title={t("settings.removeCustomAgent")}
          >
            <Trash2 className="h-3 w-3" />
          </button>
        )}

        <ToggleSwitch
          className="mt-0.5"
          checked={agent.installed && agent.enabled}
          disabled={!agent.installed}
          loading={togglingTools.has(agent.key)}
          onChange={() => handleToggleTool(agent.key, !agent.enabled)}
          title={
            !agent.installed
              ? (t("settings.notInstalled") as string)
              : agent.enabled
                ? (t("settings.disableAgent") as string)
                : (t("settings.enableAgent") as string)
          }
        />
      </div>

      <div className="space-y-1">
        {/* Global skills path */}
        {editingPathKey === agent.key ? (
          <div className="flex items-center gap-1">
            <input
              type="text"
              value={editingPathValue}
              onChange={(e) => setEditingPathValue(e.target.value)}
              className="h-7 min-w-0 flex-1 rounded border border-border-subtle bg-background px-1.5 text-[12px] font-mono text-secondary outline-none focus:border-accent"
              autoFocus
              onKeyDown={(e) => {
                if (e.key === "Enter") handleSavePath();
                if (e.key === "Escape") setEditingPathKey(null);
              }}
            />
            <button
              onClick={() => handleBrowsePath(setEditingPathValue)}
              className="shrink-0 p-1 text-muted hover:text-accent outline-none"
              title={t("settings.selectFolder")}
            >
              <FolderOpen className="h-3 w-3" />
            </button>
            <button
              onClick={handleSavePath}
              className="shrink-0 p-1 text-emerald-500 hover:text-emerald-400 outline-none"
            >
              <Check className="h-3 w-3" />
            </button>
            <button
              onClick={() => setEditingPathKey(null)}
              className="shrink-0 p-1 text-muted hover:text-secondary outline-none"
            >
              <X className="h-3 w-3" />
            </button>
          </div>
        ) : (
          <div className="flex items-center gap-1">
            <p
              className="min-w-0 flex-1 truncate text-[12px] font-mono leading-tight text-muted"
              title={agent.skills_dir}
            >
              {compactHomePath(agent.skills_dir)}
            </p>
            <button
              type="button"
              onClick={() => startEditPath(agent.key, agent.skills_dir)}
              className="shrink-0 p-0.5 text-muted hover:text-accent outline-none opacity-0 transition-opacity group-hover:opacity-100"
              title={t("settings.editPath")}
            >
              <Pencil className="h-3 w-3" />
            </button>
            {agent.has_path_override && !agent.is_custom && (
              <button
                type="button"
                onClick={() => handleResetPath(agent.key)}
                className="shrink-0 p-0.5 text-muted hover:text-amber-500 outline-none opacity-0 transition-opacity group-hover:opacity-100"
                title={t("settings.resetPath")}
              >
                <RotateCcw className="h-3 w-3" />
              </button>
            )}
          </div>
        )}

        {/* Project-relative skills path — always rendered so every card is the
            same height, installed or not. */}
        {editingProjectPathKey === agent.key ? (
            <div className="flex items-center gap-1">
              <input
                type="text"
                value={editingProjectPathValue}
                onChange={(e) => setEditingProjectPathValue(e.target.value)}
                placeholder={t("settings.projectSkillsPathPlaceholder")}
                className="h-7 min-w-0 flex-1 rounded border border-border-subtle bg-background px-1.5 text-[12px] font-mono text-secondary outline-none focus:border-accent"
                autoFocus
                onKeyDown={(e) => {
                  if (e.key === "Enter") handleSaveProjectPath();
                  if (e.key === "Escape") setEditingProjectPathKey(null);
                }}
              />
              <button
                onClick={handleSaveProjectPath}
                className="shrink-0 p-1 text-emerald-500 hover:text-emerald-400 outline-none"
              >
                <Check className="h-3 w-3" />
              </button>
              <button
                onClick={() => setEditingProjectPathKey(null)}
                className="shrink-0 p-1 text-muted hover:text-secondary outline-none"
              >
                <X className="h-3 w-3" />
              </button>
            </div>
          ) : (
            <div className="flex items-center gap-1">
              <p
                className="min-w-0 flex-1 truncate text-[12px] font-mono leading-tight text-muted"
                title={agent.project_relative_skills_dir ?? t("settings.projectSkillsPathDesc")}
              >
                {agent.project_relative_skills_dir
                  ? !agent.is_custom && !agent.has_project_path_override
                    ? t("settings.projectSkillsPathDefault", {
                        path: agent.project_relative_skills_dir,
                      })
                    : t("settings.projectSkillsPathValue", {
                        path: agent.project_relative_skills_dir,
                      })
                  : t("settings.projectSkillsPathEmpty")}
              </p>
              <button
                type="button"
                onClick={() =>
                  startEditProjectPath(agent.key, agent.project_relative_skills_dir)
                }
                className="shrink-0 p-0.5 text-muted hover:text-accent outline-none opacity-0 transition-opacity group-hover:opacity-100"
                title={t("settings.editPath")}
              >
                <Pencil className="h-3 w-3" />
              </button>
              {!agent.is_custom && agent.has_project_path_override && (
                <button
                  type="button"
                  onClick={() => handleResetProjectPath(agent.key)}
                  className="shrink-0 p-0.5 text-muted hover:text-amber-500 outline-none opacity-0 transition-opacity group-hover:opacity-100"
                  title={t("settings.resetPath")}
                >
                  <RotateCcw className="h-3 w-3" />
                </button>
              )}
            </div>
          )}
      </div>
    </div>
  );

  return (
    <div className="app-page app-page-narrow">
      <div className="app-page-header">
        <h1 className="app-page-title flex items-center gap-2">
          <Settings2 className="w-4 h-4 text-accent" />
          {t("settings.title")}
        </h1>
      </div>

      <div className="space-y-6">
        {/* Agent status */}
        <section>
          <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
            <div>
              <h2 className="app-section-title">
                {t("settings.supportedAgents")} ({installedTools.length}/{tools.length})
              </h2>
            </div>
            <div className="flex flex-wrap items-center gap-3">
              <button
                onClick={() => setShowAddCustom(true)}
                className="flex items-center gap-1 text-[13px] text-accent hover:text-accent-light transition-colors font-medium outline-none"
              >
                <Plus className="w-3.5 h-3.5" />
                {t("settings.addCustomAgent")}
              </button>
              <button
                onClick={() => handleToggleAllTools(true)}
                className="text-[13px] text-accent hover:text-accent-light transition-colors font-medium outline-none"
              >
                {t("settings.enableAll")}
              </button>
              <button
                onClick={() => handleToggleAllTools(false)}
                className="text-[13px] text-muted hover:text-secondary transition-colors font-medium outline-none"
              >
                {t("settings.disableAll")}
              </button>
              <button
                onClick={handleRefresh}
                disabled={refreshing}
                className="flex items-center gap-1.5 text-[13px] text-accent hover:text-accent-light transition-colors font-medium outline-none"
              >
                {refreshing ? (
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                ) : (
                  <RefreshCw className="w-3.5 h-3.5" />
                )}
                {t("settings.refresh")}
              </button>
            </div>
          </div>

          <div className="mb-3 flex flex-wrap items-center gap-3 text-[13px] text-muted">
            <span>{t("settings.detectedAgents")} <span className="font-medium text-secondary">{installedTools.length}</span></span>
            <span>{t("settings.enabledAgents")} <span className="font-medium text-secondary">{enabledTools.length}</span></span>
            <span>{t("settings.customAgents")} <span className="font-medium text-secondary">{customTools.length}</span></span>
          </div>

          {/* Add custom agent form */}
          {showAddCustom && (
            <div className="app-panel p-4 mb-3 space-y-2.5">
              <div className="flex items-center justify-between">
                <h3 className="text-[13px] font-medium text-secondary">{t("settings.addCustomAgent")}</h3>
                <button onClick={() => setShowAddCustom(false)} className="text-muted hover:text-secondary outline-none">
                  <X className="w-3.5 h-3.5" />
                </button>
              </div>
              <div>
                <label className="text-[12px] text-muted mb-1 block">{t("settings.agentName")}</label>
                <input
                  type="text"
                  value={customName}
                  onChange={(e) => setCustomName(e.target.value)}
                  placeholder={t("settings.agentNamePlaceholder")}
                  className={`${fieldClass} w-full`}
                />
              </div>
              <div>
                <label className="text-[12px] text-muted mb-1 block">{t("settings.skillsPath")}</label>
                <div className="flex flex-wrap items-center gap-2">
                  <input
                    type="text"
                    value={customPath}
                    onChange={(e) => setCustomPath(e.target.value)}
                    placeholder={t("settings.skillsPathPlaceholder")}
                    className={`${fieldClass} min-w-0 flex-1 font-mono`}
                  />
                  <button
                    onClick={() => handleBrowsePath(setCustomPath)}
                    className={`${actionButtonClass} bg-surface-hover hover:bg-surface-active text-tertiary border-border`}
                  >
                    <FolderOpen className="w-3 h-3" />
                    {t("settings.selectFolder")}
                  </button>
                </div>
              </div>
              <div>
                <label className="text-[12px] text-muted mb-1 block">
                  {t("settings.projectSkillsPath")}
                </label>
                <input
                  type="text"
                  value={customProjectPath}
                  onChange={(e) => setCustomProjectPath(e.target.value)}
                  placeholder={t("settings.projectSkillsPathPlaceholder")}
                  className={`${fieldClass} w-full font-mono`}
                />
                <p className="mt-1 text-[12px] text-muted">
                  {t("settings.projectSkillsPathDesc")}
                </p>
              </div>
              <div className="flex justify-end">
                <button
                  onClick={handleAddCustomAgent}
                  disabled={addingCustom || !customName.trim() || !customPath.trim()}
                  className={`${actionButtonClass} bg-accent text-white border-accent hover:opacity-90 disabled:opacity-50`}
                >
                  {addingCustom ? <Loader2 className="w-3 h-3 animate-spin" /> : <Plus className="w-3 h-3" />}
                  {t("settings.addAgent")}
                </button>
              </div>
            </div>
          )}

          <div className="space-y-4">
            {detectedTools.length > 0 && (
              <div>
                <div className="mb-2 flex items-center justify-between gap-2">
                  <h3 className="text-[13px] font-medium text-secondary">{t("settings.detectedAgentsSection")}</h3>
                  <span className="text-[12px] text-muted tabular-nums">{detectedTools.length}</span>
                </div>
                <AgentGroupDnd
                  items={detectedTools}
                  sensors={dragSensors}
                  dragLabel={t("settings.dragToReorder")}
                  onDragEnd={handleAgentDragEnd}
                  renderAgentCard={renderAgentCard}
                />
              </div>
            )}

            {undetectedTools.length > 0 && (
              <div>
                <button
                  type="button"
                  onClick={() => setShowMoreAgents((value) => !value)}
                  className="mb-2 inline-flex items-center gap-1.5 text-[13px] font-medium text-muted transition-colors hover:text-secondary outline-none"
                >
                  {showMoreAgents ? <ChevronDown className="h-3.5 w-3.5" /> : <ChevronRight className="h-3.5 w-3.5" />}
                  {t("settings.otherAgentsSection", { count: undetectedTools.length })}
                </button>
                {showMoreAgents && (
                  <AgentGroupDnd
                    items={undetectedTools}
                    sensors={dragSensors}
                    dragLabel={t("settings.dragToReorder")}
                    onDragEnd={handleAgentDragEnd}
                    renderAgentCard={renderAgentCard}
                  />
                )}
              </div>
            )}

            {customTools.length > 0 && (
              <div>
                <div className="mb-2 flex items-center justify-between gap-2">
                  <h3 className="text-[13px] font-medium text-secondary">{t("settings.customAgentsSection")}</h3>
                  <span className="text-[12px] text-muted">{customTools.length}</span>
                </div>
                <AgentGroupDnd
                  items={customTools}
                  sensors={dragSensors}
                  dragLabel={t("settings.dragToReorder")}
                  onDragEnd={handleAgentDragEnd}
                  renderAgentCard={renderAgentCard}
                />
              </div>
            )}
          </div>
        </section>

        {/* Custom read-only discovery paths */}
        <section>
          <h2 className="app-section-title mb-3">{t("settings.readOnlyPaths")}</h2>
          <div className="app-panel px-5 py-4">
            <p className="mt-0.5 text-[12px] text-muted">{t("settings.readOnlyPathsDesc")}</p>
            <div className="mt-3 space-y-2">
              {readOnlyPaths.map((path) => (
                <div key={path} className="flex items-center gap-2 rounded-md border border-border-subtle bg-background px-2.5 py-2">
                  <FolderOpen className="h-3.5 w-3.5 shrink-0 text-muted" />
                  <span className="min-w-0 flex-1 truncate font-mono text-[12px] text-secondary" title={path}>{path}</span>
                  <button
                    type="button"
                    onClick={() => void handleRemoveReadOnlyPath(path)}
                    disabled={savingReadOnlyPaths}
                    className="rounded p-1 text-muted hover:bg-red-500/10 hover:text-red-500 disabled:opacity-50"
                    title={t("settings.removeReadOnlyPath")}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </button>
                </div>
              ))}
              <div className="flex min-w-0 items-center gap-2">
                <input
                  value={readOnlyPathInput}
                  onChange={(e) => setReadOnlyPathInput(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") void handleAddReadOnlyPath();
                  }}
                  placeholder={t("settings.readOnlyPathPlaceholder")}
                  className={`${fieldClass} min-w-0 flex-1 font-mono`}
                />
                <button
                  type="button"
                  onClick={() => void handleBrowseReadOnlyPath()}
                  disabled={savingReadOnlyPaths}
                  className={`${actionButtonClass} text-muted hover:text-secondary`}
                >
                  <FolderOpen className="h-3 w-3" />
                </button>
                <button
                  type="button"
                  onClick={() => void handleAddReadOnlyPath()}
                  disabled={savingReadOnlyPaths || !readOnlyPathInput.trim()}
                  className={`${actionButtonClass} border-accent-border bg-accent-dark text-white hover:bg-accent disabled:opacity-50`}
                >
                  <Plus className="h-3 w-3" />
                  {t("settings.addReadOnlyPath")}
                </button>
              </div>
            </div>
          </div>
        </section>

        {/* Global config */}
        <section>
          <h2 className="app-section-title mb-3">
            {t("settings.globalConfig")}
          </h2>
          <div className="app-panel overflow-hidden divide-y divide-border-faint">
            {/* Repo path */}
            <div className="flex flex-wrap items-start justify-between gap-3 px-5 py-4">
              <div className="min-w-0 flex-1">
                <h3 className="text-[14px] font-semibold text-primary">{t("settings.repoPath")}</h3>
                <p className="mt-0.5 text-[12px] text-muted">{t("settings.repoPathDesc")}</p>
              </div>
              <div className="flex max-w-full flex-wrap items-center gap-2">
                {editingCentralRepoPath ? (
                  <div className="flex min-w-[320px] max-w-full items-center gap-1">
                    <input
                      type="text"
                      value={centralRepoPathInput}
                      onChange={(e) => setCentralRepoPathInput(e.target.value)}
                      className={`${fieldClass} min-w-0 flex-1 font-mono`}
                      autoFocus
                      onKeyDown={(e) => {
                        if (e.key === "Enter") void handleSaveCentralRepoPath();
                        if (e.key === "Escape") {
                          setCentralRepoPathInput(centralRepoPathOverride ?? centralRepoPath);
                          setEditingCentralRepoPath(false);
                        }
                      }}
                    />
                    <button
                      type="button"
                      onClick={() => handleBrowsePath(setCentralRepoPathInput)}
                      disabled={savingCentralRepoPath}
                      className={`${actionButtonClass} text-muted hover:text-secondary`}
                    >
                      <FolderOpen className="w-3 h-3" />
                      {t("settings.selectFolder")}
                    </button>
                    <button
                      type="button"
                      onClick={() => void handleSaveCentralRepoPath()}
                      disabled={savingCentralRepoPath}
                      className={`${actionButtonClass} border-emerald-500/30 text-emerald-600 hover:bg-emerald-500/5 dark:text-emerald-400`}
                    >
                      {savingCentralRepoPath ? (
                        <Loader2 className="w-3 h-3 animate-spin" />
                      ) : (
                        <Check className="w-3 h-3" />
                      )}
                      {t("common.save")}
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setCentralRepoPathInput(centralRepoPathOverride ?? centralRepoPath);
                        setEditingCentralRepoPath(false);
                      }}
                      disabled={savingCentralRepoPath}
                      className={`${actionButtonClass} text-muted hover:text-secondary`}
                    >
                      <X className="w-3 h-3" />
                    </button>
                  </div>
                ) : (
                  <div className="flex min-w-0 items-center gap-1.5 rounded-lg border border-border-subtle bg-background px-3 py-2">
                    <Folder className="w-3 h-3 text-muted" />
                    <span className="truncate text-[13px] font-mono text-tertiary">{displayedRepoPath}</span>
                  </div>
                )}
                {!editingCentralRepoPath && (
                  <button
                    type="button"
                    onClick={handleStartEditCentralRepoPath}
                    className={`${actionButtonClass} text-muted hover:text-secondary`}
                  >
                    <Pencil className="w-3 h-3" />
                    {t("settings.changeDir")}
                  </button>
                )}
                {!editingCentralRepoPath && centralRepoPathOverride && (
                  <button
                    type="button"
                    onClick={() => void handleResetCentralRepoPath()}
                    disabled={savingCentralRepoPath}
                    className={`${actionButtonClass} text-muted hover:text-secondary`}
                  >
                    {savingCentralRepoPath ? (
                      <Loader2 className="w-3 h-3 animate-spin" />
                    ) : (
                      <RotateCcw className="w-3 h-3" />
                    )}
                    {t("settings.resetPath")}
                  </button>
                )}
                <button
                  type="button"
                  onClick={handleOpenRepoInFinder}
                  disabled={openingRepo}
                  className={cn(
                    actionButtonClass,
                    "border-accent-border bg-accent-bg text-accent",
                    "hover:border-accent hover:bg-accent-bg",
                    openingRepo && "cursor-wait opacity-70"
                  )}
                >
                  {openingRepo ? (
                    <Loader2 className="w-3 h-3 animate-spin" />
                  ) : (
                    <ExternalLink className="w-3 h-3" />
                  )}
                  {t("settings.openInFinder")}
                </button>
              </div>
              <div className="w-full text-[12px] text-muted">
                {centralRepoPathOverride
                  ? t("settings.repoPathCustomHint")
                  : t("settings.repoPathDefaultHint")}
              </div>
            </div>

            {/* Theme */}
            <div className="flex flex-wrap items-start justify-between gap-3 px-5 py-4">
              <div className="min-w-0 flex-1">
                <h3 className="text-[14px] font-semibold text-primary">{t("settings.theme")}</h3>
                <p className="mt-0.5 text-[12px] text-muted">{t("settings.themeDesc")}</p>
              </div>
              <div className="app-segmented flex-wrap bg-background">
                {themeOptions.map((opt) => {
                  const Icon = opt.icon;
                  return (
                    <button
                      key={opt.value}
                      onClick={() => setTheme(opt.value)}
                      className={cn(
                        segmentedButtonClass,
                        theme === opt.value ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
                      )}
                    >
                      <Icon className="w-3 h-3" /> {opt.label}
                    </button>
                  );
                })}
              </div>
            </div>

            {/* Text size */}
            <div className="flex flex-wrap items-start justify-between gap-3 px-5 py-4">
              <div className="min-w-0 flex-1">
                <h3 className="text-[14px] font-semibold text-primary">{t("settings.textSize")}</h3>
                <p className="mt-0.5 text-[12px] text-muted">{t("settings.textSizeDesc")}</p>
              </div>
              <div className="app-segmented flex-wrap bg-background">
                {([
                  { value: "small", label: t("settings.textSizeSmall") },
                  { value: "default", label: t("settings.textSizeDefault") },
                  { value: "large", label: t("settings.textSizeLarge") },
                  { value: "xlarge", label: t("settings.textSizeXLarge") },
                ] as const).map((opt) => (
                  <button
                    key={opt.value}
                    onClick={() => handleTextSizeChange(opt.value)}
                    className={cn(
                      segmentedButtonClass,
                      textSize === opt.value ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
                    )}
                  >
                    {opt.value === "small" && <Type className="w-2.5 h-2.5" />}
                    {opt.value === "default" && <Type className="w-3 h-3" />}
                    {opt.value === "large" && <Type className="w-3.5 h-3.5" />}
                    {opt.value === "xlarge" && <Type className="w-4 h-4" />}
                    {opt.label}
                  </button>
                ))}
              </div>
            </div>

            {/* Language */}
            <div className="flex flex-wrap items-start justify-between gap-3 px-5 py-4">
              <div className="min-w-0 flex-1">
                <h3 className="text-[14px] font-semibold text-primary">{t("settings.language")}</h3>
              </div>
              <div className="app-segmented flex-wrap bg-background">
                {([
                  { value: "zh", label: "简体中文" },
                  { value: "en", label: "English" },
                ] as const).map((opt) => (
                  <button
                    key={opt.value}
                    onClick={() => handleLanguageChange(opt.value)}
                    className={cn(
                      segmentedButtonClass,
                      i18n.language === opt.value
                        ? "bg-surface-active text-secondary"
                        : "text-muted hover:text-tertiary"
                    )}
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
            </div>
          </div>
        </section>

        {/* Proxy config */}
        <section>
          <h2 className="app-section-title mb-3">
            {t("settings.proxyConfig")}
          </h2>
          <div className="app-panel overflow-hidden divide-y divide-border-faint">
            <div className="px-4 py-3">
              <h3 className="text-[14px] font-semibold text-primary">{t("settings.proxyUrl")}</h3>
              <p className="mt-0.5 mb-2 text-[12px] text-muted">{t("settings.proxyUrlDesc")}</p>
              <div className="flex flex-wrap items-center gap-2">
                <input
                  type="text"
                  value={proxyInput}
                  onChange={(e) => setProxyInput(e.target.value)}
                  placeholder={t("settings.proxyUrlPlaceholder")}
                  className={`${fieldClass} min-w-0 flex-1 font-mono`}
                />
                <button
                  onClick={handleSaveProxy}
                  disabled={proxySaving}
                  className={`${actionButtonClass} bg-surface-hover hover:bg-surface-active text-tertiary border-border`}
                >
                  {proxySaving ? (
                    <Loader2 className="w-3 h-3 animate-spin" />
                  ) : (
                    <LinkIcon className="w-3 h-3" />
                  )}
                  {t("common.save")}
                </button>
              </div>
            </div>
          </div>
        </section>

        {/* About */}
        <section className="space-y-2">
          {repoWarnings.length > 0 && (
            <div className="app-panel flex flex-wrap items-start gap-2 p-3 border border-amber-500/40 bg-amber-500/10">
              <AlertTriangle className="w-4 h-4 shrink-0 mt-0.5 text-amber-700 dark:text-amber-300" />
              <div className="min-w-0 flex-1 space-y-1 text-[13px] text-amber-800 dark:text-amber-300">
                {repoWarnings.map((code) => (
                  <p key={code}>{t(`settings.repoWarning_${code}`)}</p>
                ))}
              </div>
            </div>
          )}
          {lastPanic && (
            <div className="app-panel flex flex-wrap items-center justify-between gap-2 p-3 border border-red-500/40 bg-red-500/10">
              <div className="flex min-w-0 items-center gap-2 text-[13px] text-red-700 dark:text-red-300">
                <AlertTriangle className="w-4 h-4 shrink-0" />
                <span>{t("settings.panicBanner", { time: lastPanic.timestamp })}</span>
              </div>
              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={handleReportIssue}
                  disabled={reportingIssue}
                  className={`${actionButtonClass} bg-red-600 hover:bg-red-700 text-white border-red-600`}
                >
                  {reportingIssue ? (
                    <Loader2 className="w-3 h-3 animate-spin" />
                  ) : (
                    <Bug className="w-3 h-3" />
                  )}
                  {t("settings.reportIssue")}
                </button>
                <button
                  type="button"
                  onClick={handleDismissPanic}
                  className={`${actionButtonClass} bg-surface-hover hover:bg-surface-active text-tertiary border-border`}
                >
                  {t("settings.panicDismiss")}
                </button>
              </div>
            </div>
          )}
          <div className="app-panel flex flex-wrap items-start justify-between gap-3 p-4">
            <div className="flex min-w-0 flex-1 items-center gap-3">
              <div className="w-8 h-8 rounded-lg bg-surface-hover border border-border flex items-center justify-center">
                <Settings2 className="w-4 h-4 text-accent" />
              </div>
              <div>
                <h3 className="text-[13px] font-semibold text-primary">{t("settings.version")}</h3>
                <p className="text-muted text-[13px]">{t("settings.tagline")}</p>
              </div>
            </div>
            <div className="flex flex-wrap gap-2">
              <button
                type="button"
                onClick={openHelp}
                className={`${actionButtonClass} bg-surface-hover hover:bg-surface-active text-tertiary border-border`}
              >
                <BookOpen className="w-3 h-3" /> {t("settings.help")}
              </button>
              <button
                type="button"
                onClick={handleReportIssue}
                disabled={reportingIssue}
                title={t("settings.reportIssueHint")}
                className={`${actionButtonClass} bg-surface-hover hover:bg-surface-active text-tertiary border-border`}
              >
                {reportingIssue ? (
                  <Loader2 className="w-3 h-3 animate-spin" />
                ) : (
                  <Bug className="w-3 h-3" />
                )}
                {t("settings.reportIssue")}
              </button>
              <button
                type="button"
                onClick={handleExportLogs}
                disabled={exportingLogs}
                title={t("settings.exportLogsHint")}
                className={`${actionButtonClass} bg-surface-hover hover:bg-surface-active text-tertiary border-border`}
              >
                {exportingLogs ? (
                  <Loader2 className="w-3 h-3 animate-spin" />
                ) : (
                  <FileArchive className="w-3 h-3" />
                )}
                {t("settings.exportLogs")}
              </button>
              <button
                type="button"
                onClick={() => { openUrl(WEBSITE_URL).catch(() => {}); }}
                className={`${actionButtonClass} bg-surface-hover hover:bg-surface-active text-tertiary border-border`}
              >
                <Globe className="w-3 h-3" /> {t("settings.website")}
              </button>
              <button
                type="button"
                onClick={handleOpenGithub}
                disabled={openingGithub}
                className={`${actionButtonClass} bg-surface-hover hover:bg-surface-active text-tertiary border-border`}
              >
                <Github className="w-3 h-3" /> GitHub
              </button>
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}
