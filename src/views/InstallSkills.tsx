import { useState, useEffect, useCallback, useRef } from "react";
import {
  DownloadCloud,
  UploadCloud,
  Github,
  FolderUp,
  Loader2,
  Check,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { cn } from "../utils";
import { useApp } from "../context/AppContext";
import { useCurrentProject } from "../lib/useCurrentProject";
import * as api from "../lib/tauri";
import type { GitPreviewResult, GitInstallOutcome, GitInstallScope } from "../lib/tauri";
import { open } from "@tauri-apps/plugin-dialog";
import { useSearchParams, useNavigate } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { StatusBanner } from "../components/StatusBanner";
import { getErrorMessage, getErrorKind } from "../lib/error";

/** Merge fresh outcomes into previous ones by rel_path (untouched rows keep theirs). */
function mergeOutcomes(
  prev: GitInstallOutcome[] | null,
  fresh: GitInstallOutcome[]
): GitInstallOutcome[] {
  const merged = new Map((prev ?? []).map((o) => [o.rel_path, o]));
  for (const outcome of fresh) merged.set(outcome.rel_path, outcome);
  return [...merged.values()];
}

export function InstallSkills() {
  const { t } = useTranslation();
  const { refreshPresets, refreshManagedSkills, managedSkills, openSkillDetailById } = useApp();
  const { currentProject } = useCurrentProject();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const [activeTab, setActiveTab] = useState<"local" | "git">("git");
  const [gitUrl, setGitUrl] = useState("");
  const [gitLoading, setGitLoading] = useState(false);
  const [gitCancelKey, setGitCancelKey] = useState<string | null>(null);
  const [gitPreview, setGitPreview] = useState<GitPreviewResult | null>(null);
  const [gitPreviewRepoUrl, setGitPreviewRepoUrl] = useState<string | null>(null);
  const [gitSelections, setGitSelections] = useState<{ rel_path: string; name: string; description: string | null; selected: boolean }[]>([]);
  const [gitScope, setGitScope] = useState<GitInstallScope>("user");
  const [gitOutcomes, setGitOutcomes] = useState<GitInstallOutcome[] | null>(null);
  const [gitConfirmLoading, setGitConfirmLoading] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);

  const managedSkillsRef = useRef(managedSkills);
  managedSkillsRef.current = managedSkills;

  const goToSkill = useCallback((skillName: string) => {
    // Use ref to get the latest managedSkills after refresh
    const skills = managedSkillsRef.current;
    const skill = skills.find(
      (s) => s.name === skillName || s.source_ref === skillName
    );
    if (skill) {
      openSkillDetailById(skill.id);
    }
    navigate("/my-skills");
  }, [navigate, openSkillDetailById]);

  const findInstalledByGitUrl = useCallback((url: string) => {
    const trimmed = url.trim().replace(/\.git$/, "").toLowerCase();
    return managedSkills.find((s) => {
      if (!s.source_ref) return false;
      const ref = s.source_ref.replace(/\.git$/, "").toLowerCase();
      return ref === trimmed || ref.endsWith("/" + trimmed.split("/").slice(-2).join("/"));
    });
  }, [managedSkills]);

  useEffect(() => {
    const tab = searchParams.get("tab");
    if (tab === "local" || tab === "git") {
      setActiveTab(tab);
    }
  }, [searchParams]);

  const switchTab = (tab: "local" | "git") => {
    setActiveTab(tab);
    setSearchParams({ tab });
  };

  const installLocalSource = async (sourcePath: string) => {
    const name = sourcePath.split("/").pop() || sourcePath;
    const toastId = toast.loading(t("install.toast.installing", { name }));
    try {
      await api.installLocal(sourcePath);
    } catch (e) {
      const message = getErrorMessage(e, t("common.error"));
      setLocalError(message);
      toast.error(message, { id: toastId });
      return;
    }
    // Install succeeded — post-install refresh is best-effort and must not
    // surface as an install failure.
    await Promise.allSettled([
      refreshPresets(),
      refreshManagedSkills(),
    ]);
    toast.success(t("install.toast.success", { name }), {
      id: toastId,
      action: {
        label: t("install.toast.view"),
        onClick: () => goToSkill(name),
      },
    });
  };

  const handleLocalFolderInstall = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
      });
      if (!selected) return;
      installLocalSource(selected as string);
    } catch (error: unknown) {
      const message = getErrorMessage(error, t("common.error"));
      setLocalError(message);
      toast.error(message);
    }
  };

  const handleLocalFileInstall = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "Skills", extensions: ["zip", "skill"] }],
      });
      if (!selected) return;
      installLocalSource(selected as string);
    } catch (error: unknown) {
      const message = getErrorMessage(error, t("common.error"));
      setLocalError(message);
      toast.error(message);
    }
  };

  const handleCancelInstall = (cancelKey: string) => {
    api.cancelInstall(cancelKey).catch(() => {
      // Ignore race: install may have completed before cancel request arrives.
    });
  };

  const handleGitPreview = async () => {
    if (!gitUrl.trim()) return;
    // A previous preview's temp must not leak when starting over.
    cancelPreviewTemp();
    setGitLoading(true);
    const url = gitUrl.trim();
    setGitCancelKey(url);

    const toastId = toast.loading(t("install.toast.cloning"));
    let unlisten: (() => void) | null = null;

    try {
      unlisten = await listen<{ skill_id: string; phase: string; detail?: string }>(
        "install-progress",
        (event) => {
          if (event.payload.skill_id !== url) return;
          if (event.payload.phase === "cloning") {
            const detail = event.payload.detail?.trim();
            const msg = detail
              ? `${t("install.toast.cloning")}\n${detail}`
              : t("install.toast.cloning");
            toast.loading(msg, { id: toastId });
          }
        }
      );
      const preview = await api.previewGitInstall(url);
      toast.dismiss(toastId);
      setGitPreview(preview);
      setGitPreviewRepoUrl(url);
      setGitOutcomes(null);
      setGitSelections(preview.skills.map((s) => ({
        rel_path: s.rel_path,
        name: s.name,
        description: s.description,
        selected: true,
      })));
    } catch (error: unknown) {
      if (getErrorKind(error) === "cancelled") {
        toast.info(t("install.toast.cancelled"), { id: toastId });
      } else {
        toast.error(getErrorMessage(error, t("common.error")), { id: toastId });
      }
    } finally {
      setGitLoading(false);
      setGitCancelKey(null);
      unlisten?.();
    }
  };

  const handleGitPreviewClose = () => {
    if (gitConfirmLoading) return;
    closeGitDialog();
  };

  // The preview temp stays alive across conflict retries: the backend only
  // cleans it when nothing conflicted, and every exit below cancels it.
  const cancelPreviewTemp = useCallback(() => {
    if (gitPreview) {
      api.cancelGitPreview(gitPreview.temp_dir).catch(() => {});
    }
  }, [gitPreview]);

  const closeGitDialog = useCallback(() => {
    cancelPreviewTemp();
    setGitPreview(null);
    setGitPreviewRepoUrl(null);
    setGitSelections([]);
    setGitOutcomes(null);
  }, [cancelPreviewTemp]);

  const handleGitConfirm = async (replace = false) => {
    if (!gitPreview) return;
    const repoUrl = gitPreviewRepoUrl ?? gitUrl.trim();
    if (!repoUrl) return;
    // Retry semantics once outcomes exist:
    // - Replace → conflicts only (installed rows must not be reinstalled).
    // - Import Selected → selected rows except already-installed ones, so a
    //   retry after a partial batch never resubmits successes or a deleted
    //   temp's ghosts.
    const candidates = gitSelections.filter((s) => s.selected);
    const outcomeOf = (relPath: string) =>
      gitOutcomes?.find((o) => o.rel_path === relPath)?.status;
    const selected = !gitOutcomes
      ? candidates
      : replace
        ? candidates.filter((s) => outcomeOf(s.rel_path) === "conflict")
        : candidates.filter((s) => outcomeOf(s.rel_path) !== "installed");
    if (selected.length === 0) return;
    if (gitScope === "project" && !currentProject) {
      toast.error(t("install.gitPreview.noProject"));
      return;
    }
    setGitConfirmLoading(true);
    try {
      const result = await api.confirmGitInstall(
        repoUrl,
        gitPreview.temp_dir,
        selected.map((s) => ({ rel_path: s.rel_path, name: s.name })),
        gitScope,
        currentProject?.id ?? null,
        replace
      );
      // Merge by rel_path: rows this retry did not touch keep their outcome
      // (a previous failure must not vanish from the dialog).
      setGitOutcomes((prev) => {
        const merged = new Map((prev ?? []).map((o) => [o.rel_path, o]));
        for (const outcome of result.outcomes) merged.set(outcome.rel_path, outcome);
        return [...merged.values()];
      });
      const merged = mergeOutcomes(gitOutcomes, result.outcomes);
      const installed = merged.filter((o) => o.status === "installed");
      const conflicts = merged.filter((o) => o.status === "conflict");
      const failed = merged.filter((o) => o.status === "failed");
      await Promise.all([refreshPresets(), refreshManagedSkills()]);
      if (conflicts.length === 0 && failed.length === 0) {
        toast.success(t("install.toast.success", { name: installed.map((s) => s.name).join(", ") }));
        setGitUrl("");
        closeGitDialog();
      } else {
        const parts = [`${installed.length} installed`];
        if (conflicts.length > 0) parts.push(`${conflicts.length} already managed`);
        if (failed.length > 0) parts.push(`${failed.length} failed`);
        toast.warning(parts.join(", "));
        // Dialog stays open on the live temp: conflicts retry with Replace.
      }
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      setGitConfirmLoading(false);
    }
  };

  return (
    <div className="app-page gap-4">
      <div className="app-page-header border-b-0 pb-0">
        <h1 className="app-page-title mb-4">{t("install.title")}</h1>
        <div className="flex gap-1 border-b border-border-subtle">
          {[
            { id: "git" as const, label: t("install.gitInstall"), icon: Github },
            { id: "local" as const, label: t("install.localInstall"), icon: UploadCloud },
          ].map((tab) => {
            const Icon = tab.icon;
            const isActive = activeTab === tab.id;
            return (
              <button
                key={tab.id}
                onClick={() => switchTab(tab.id)}
                className={cn(
                  "mr-4 flex items-center gap-1.5 border-b-2 px-1 pb-1.5 text-[13px] font-medium transition-colors outline-none",
                  isActive
                    ? "border-accent text-accent"
                    : "border-transparent text-muted hover:text-tertiary"
                )}
              >
                <Icon className="h-3.5 w-3.5" />
                {tab.label}
              </button>
            );
          })}
        </div>
      </div>

      {activeTab === "local" && (
        <div className="space-y-4 pb-8 animate-in fade-in duration-300">
          <section className="app-panel overflow-hidden">
            <div className="border-b border-border-subtle px-4 py-3.5">
              <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
                <div className="max-w-xl">
                  <div className="mb-2 flex flex-wrap items-center gap-2 text-[13px] text-muted">
                    <span className="inline-flex items-center gap-1.5 rounded-[5px] border border-accent-border bg-accent-bg px-2 py-1 font-medium text-accent-light">
                      <FolderUp className="h-3.5 w-3.5" />
                      {t("install.local.title")}
                    </span>
                  </div>

                  <h2 className="text-[14px] font-semibold text-secondary">
                    {t("install.local.title")}
                  </h2>
                  <p className="mt-1 text-[13px] leading-5 text-muted">
                    {t("install.local.description")}
                  </p>
                </div>

                <div className="flex flex-wrap gap-2">
                  <button
                    type="button"
                    onClick={handleLocalFolderInstall}
                    className="app-button-primary"
                  >
                    <FolderUp className="h-4 w-4" />
                    {t("install.local.selectFolder")}
                  </button>
                  <button
                    type="button"
                    onClick={handleLocalFileInstall}
                    className="app-button-secondary bg-background"
                  >
                    <UploadCloud className="h-4 w-4" />
                    {t("install.local.selectArchive")}
                  </button>
                </div>
              </div>
            </div>

          </section>

          {localError ? (
            <StatusBanner
              compact
              title={t("common.requestFailed")}
              description={localError}
              tone="danger"
            />
          ) : null}
        </div>
      )}

      {activeTab === "git" && (
        <div className="animate-in fade-in duration-300">
          <div className="app-panel max-w-lg p-5">
            <div className="mb-4 flex h-10 w-10 items-center justify-center rounded-lg border border-border bg-surface-hover">
              <Github className="h-5 w-5 text-tertiary" />
            </div>
            <h2 className="mb-1 text-[14px] font-semibold text-primary">{t("install.gitTitle")}</h2>
            <p className="mb-4 text-[13px] text-muted">{t("install.gitDesc")}</p>

            <div className="space-y-3">
              <div>
                <label className="mb-1 block text-[13px] font-medium text-tertiary">
                  {t("install.repoUrl")}
                </label>
                <input
                  type="text"
                  value={gitUrl}
                  onChange={(e) => setGitUrl(e.target.value)}
                  onKeyDown={(e) => { if (e.key === "Enter" && !gitLoading && gitUrl.trim()) handleGitPreview(); }}
                  placeholder={t("install.repoUrlPlaceholder")}
                  disabled={gitLoading}
                  className="app-input w-full bg-background"
                />
              </div>
              {gitUrl.trim() && findInstalledByGitUrl(gitUrl) && (
                <div className="flex items-center gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-[13px] text-amber-400">
                  <Check className="h-3.5 w-3.5 shrink-0" />
                  <span>
                    {t("install.gitAlreadyInstalled", { name: findInstalledByGitUrl(gitUrl)!.name })}
                  </span>
                </div>
              )}
              <div className="flex gap-2 pt-2">
                {gitLoading ? (
                  <button
                    onClick={() => gitCancelKey && handleCancelInstall(gitCancelKey)}
                    className="inline-flex w-full items-center justify-center gap-2 rounded-lg border border-red-500/30 bg-red-500/10 px-4 py-2.5 text-[13px] font-medium text-red-400 transition-colors hover:bg-red-500/20"
                    disabled={!gitCancelKey}
                  >
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    {t("install.cancel")}
                  </button>
                ) : (
                  <button
                    onClick={handleGitPreview}
                    disabled={!gitUrl.trim()}
                    className={cn(
                      "flex w-full",
                      gitUrl.trim() && findInstalledByGitUrl(gitUrl)
                        ? "app-button-secondary bg-background"
                        : "app-button-primary"
                    )}
                  >
                    <DownloadCloud className="h-3.5 w-3.5" />
                    {gitUrl.trim() && findInstalledByGitUrl(gitUrl)
                      ? t("install.gitReinstall")
                      : t("install.installClone")}
                  </button>
                )}
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Git preview / selection dialog */}
      {gitPreview && (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
          <div
            className="absolute inset-0 bg-black/70 backdrop-blur-sm"
            onClick={handleGitPreviewClose}
          />
          <div className="relative w-full max-w-md rounded-xl border border-border bg-surface p-5 shadow-2xl">
            <div className="mb-4 flex items-center justify-between">
              <h2 className="text-[14px] font-semibold text-primary">{t("install.gitPreview.title")}</h2>
              <button
                onClick={handleGitPreviewClose}
                disabled={gitConfirmLoading}
                className="rounded p-1 text-muted transition-colors hover:text-secondary"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
            <p className="mb-3 text-[13px] text-muted">{t("install.gitPreview.description")}</p>

            {/* Destination: User ~/.agents/skills or current Project */}
            <div className="mb-3 flex items-center gap-2 text-[13px]">
              <span className="text-muted">{t("install.gitPreview.destination")}</span>
              <div className="app-segmented bg-background">
                <button
                  type="button"
                  onClick={() => setGitScope("user")}
                  disabled={gitConfirmLoading}
                  className={cn(
                    "app-segmented-button",
                    gitScope === "user" && "app-segmented-button-active"
                  )}
                >
                  {t("install.gitPreview.toUser")}
                </button>
                <button
                  type="button"
                  onClick={() => setGitScope("project")}
                  disabled={gitConfirmLoading || !currentProject}
                  title={currentProject ? currentProject.path : t("install.gitPreview.noProject")}
                  className={cn(
                    "app-segmented-button",
                    gitScope === "project" && "app-segmented-button-active"
                  )}
                >
                  {t("install.gitPreview.toProject")}
                </button>
              </div>
            </div>
            {gitScope === "project" && (
              <p className="mb-3 truncate text-[12px] text-muted">
                {currentProject ? `${currentProject.name} — ${currentProject.path}\\.agents\\skills` : t("install.gitPreview.noProject")}
              </p>
            )}

            {/* Select all / deselect all */}
            <div className="mb-2 flex gap-2">
              <button
                type="button"
                onClick={() => setGitSelections((prev) => prev.map((s) => ({ ...s, selected: true })))}
                disabled={gitConfirmLoading}
                className="text-[13px] text-accent-light hover:underline"
              >
                {t("install.gitPreview.selectAll")}
              </button>
              <span className="text-faint">·</span>
              <button
                type="button"
                onClick={() => setGitSelections((prev) => prev.map((s) => ({ ...s, selected: false })))}
                disabled={gitConfirmLoading}
                className="text-[13px] text-muted hover:underline"
              >
                {t("install.gitPreview.deselectAll")}
              </button>
            </div>

            {gitSelections.length === 0 ? (
              <p className="py-6 text-center text-[13px] text-muted">{t("install.gitPreview.empty")}</p>
            ) : (
              <div className="max-h-64 space-y-2 overflow-y-auto scrollbar-hide pr-1">
                {gitSelections.map((item, idx) => {
                  const outcome = gitOutcomes?.find((o) => o.rel_path === item.rel_path);
                  return (
                  <div
                    key={item.rel_path}
                    className={cn(
                      "flex items-center gap-3 rounded-lg border px-3 py-2 transition-colors",
                      item.selected
                        ? "border-accent-border bg-accent-bg/40"
                        : "border-border-subtle bg-background opacity-50"
                    )}
                  >
                    <input
                      type="checkbox"
                      checked={item.selected}
                      disabled={gitConfirmLoading}
                      onChange={(e) =>
                        setGitSelections((prev) =>
                          prev.map((s, i) => i === idx ? { ...s, selected: e.target.checked } : s)
                        )
                      }
                      className="h-4 w-4 shrink-0 accent-accent"
                    />
                    <div className="min-w-0 flex-1">
                      <input
                        type="text"
                        value={item.name}
                        onChange={(e) =>
                          setGitSelections((prev) =>
                            prev.map((s, i) => i === idx ? { ...s, name: e.target.value } : s)
                          )
                        }
                        disabled={!item.selected || gitConfirmLoading}
                        placeholder={t("install.gitPreview.namePlaceholder")}
                        className="app-input w-full bg-background py-1 text-[13px]"
                      />
                      {item.description ? (
                        <p className="mt-1 truncate text-[12px] text-muted">{item.description}</p>
                      ) : null}
                      {outcome && outcome.status !== "installed" ? (
                        <p className="mt-1 text-[12px] text-amber-400">
                          {outcome.status === "conflict"
                            ? t("install.gitPreview.alreadyManaged")
                            : outcome.error ?? t("common.error")}
                        </p>
                      ) : null}
                    </div>
                  </div>
                  );
                })}
              </div>
            )}

            <div className="mt-4 flex justify-end gap-2">
              <button
                type="button"
                onClick={handleGitPreviewClose}
                disabled={gitConfirmLoading}
                className="px-3 py-1.5 text-[13px] font-medium text-muted hover:text-secondary transition-colors"
              >
                {t("common.cancel")}
              </button>
              {gitOutcomes?.some((o) => o.status === "conflict") ? (
                <button
                  type="button"
                  onClick={() => void handleGitConfirm(true)}
                  disabled={gitConfirmLoading}
                  className="app-button-primary"
                >
                  {t("install.gitPreview.replaceConflicts")}
                </button>
              ) : null}
              <button
                type="button"
                onClick={() => void handleGitConfirm(false)}
                disabled={gitConfirmLoading || gitSelections.every((s) => !s.selected)}
                className="app-button-primary"
              >
                {gitConfirmLoading ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <DownloadCloud className="h-3.5 w-3.5" />
                )}
                {t("install.gitPreview.confirm")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
