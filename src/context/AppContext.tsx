/* eslint-disable react-refresh/only-export-components */
import { createContext, useContext, useState, useEffect, useCallback, useRef, type ReactNode } from "react";
import type { ManagedSkill, Project, Preset, ToolInfo } from "../lib/tauri";
import * as api from "../lib/tauri";
import i18n from "../i18n";
import { applyTextSize } from "../lib/textScale";

interface AppState {
  presets: Preset[];
  /** The active Preset is curation metadata; it never deploys to Harnesses. */
  activePreset: Preset | null;
  /** Frontend-only "currently being viewed/edited" preset. Persisted to localStorage. UI selection. */
  viewedPreset: Preset | null;
  tools: ToolInfo[];
  managedSkills: ManagedSkill[];
  projects: Project[];
  loading: boolean;
  appError: string | null;
  helpOpen: boolean;
  detailSkillId: string | null;
  refreshAppData: () => Promise<void>;
  refreshPresets: () => Promise<void>;
  refreshTools: () => Promise<void>;
  refreshManagedSkills: () => Promise<void>;
  refreshProjects: () => Promise<void>;
  setViewedPresetId: (id: string) => void;
  clearAppError: () => void;
  openHelp: () => void;
  closeHelp: () => void;
  openSkillDetailById: (skillId: string) => void;
  closeSkillDetail: () => void;
}

const VIEWED_PRESET_LS_KEY = "agentdock.viewedPresetId";
const LEGACY_VIEWED_PRESET_LS_KEY = "skills-manager.viewedPresetId";
const LEGACY_VIEWED_SCENARIO_LS_KEY = "skills-manager.viewedScenarioId";

const AppContext = createContext<AppState | null>(null);

export function AppProvider({ children }: { children: ReactNode }) {
  const [presets, setPresets] = useState<Preset[]>([]);
  const [activePreset, setActivePreset] = useState<Preset | null>(null);
  const [viewedPresetId, setViewedPresetIdState] = useState<string | null>(() => {
    try {
      return (
        localStorage.getItem(VIEWED_PRESET_LS_KEY) ||
        localStorage.getItem(LEGACY_VIEWED_PRESET_LS_KEY) ||
        localStorage.getItem(LEGACY_VIEWED_SCENARIO_LS_KEY)
      );
    } catch {
      return null;
    }
  });
  const [tools, setTools] = useState<ToolInfo[]>([]);
  const [managedSkills, setManagedSkills] = useState<ManagedSkill[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [loading, setLoading] = useState(true);
  const [appError, setAppError] = useState<string | null>(null);
  const [helpOpen, setHelpOpen] = useState(false);
  const [detailSkillId, setDetailSkillId] = useState<string | null>(null);
  const lastActivePresetIdRef = useRef<string | null>(null);

  const setTranslatedError = useCallback((key: string) => {
    setAppError(i18n.t("common.loadFailed", { item: i18n.t(key) }));
  }, []);

  const refreshPresets = useCallback(async () => {
    try {
      const [s, active] = await Promise.all([
        api.getPresets(),
        api.getActivePreset(),
      ]);
      setPresets(s);
      setActivePreset(active);
      const previousActiveId = lastActivePresetIdRef.current;
      const nextActiveId = active?.id ?? null;
      if (previousActiveId !== nextActiveId) {
        lastActivePresetIdRef.current = nextActiveId;
        // Carry the sidebar along only when the user was viewing the old
        // active preset — that way an external switch (e.g. CLI) follows,
        // but a user who's browsing some other preset isn't yanked away.
        // Skip the initial load (previousActiveId === null) entirely so a
        // persisted viewedPreset from localStorage isn't clobbered.
        if (nextActiveId && previousActiveId !== null) {
          setViewedPresetIdState((current) => {
            if (current !== previousActiveId) return current;
            try {
              localStorage.setItem(VIEWED_PRESET_LS_KEY, nextActiveId);
            } catch {
              // localStorage may be unavailable; selection is still tracked in memory.
            }
            return nextActiveId;
          });
        }
      }
      setAppError(null);
    } catch (e) {
      console.error("Failed to load presets:", e);
      setTranslatedError("common.presets");
    }
  }, [setTranslatedError]);

  const refreshTools = useCallback(async () => {
    try {
      const t = await api.getToolStatus();
      setTools(t);
      setAppError(null);
    } catch (e) {
      console.error("Failed to load tools:", e);
      setTranslatedError("common.agents");
    }
  }, [setTranslatedError]);

  const refreshProjects = useCallback(async () => {
    try {
      const p = await api.getProjects();
      setProjects(p);
    } catch (e) {
      console.error("Failed to load projects:", e);
    }
  }, []);

  const refreshManagedSkills = useCallback(async () => {
    try {
      const skills = await api.getManagedSkills();
      setManagedSkills(skills);
      setAppError(null);
    } catch (e) {
      console.error("Failed to load managed skills:", e);
      setTranslatedError("common.skills");
    }
  }, [setTranslatedError]);

  const refreshAppData = useCallback(async () => {
    setLoading(true);
    await Promise.all([refreshPresets(), refreshTools(), refreshManagedSkills(), refreshProjects()]);
    setLoading(false);
  }, [refreshManagedSkills, refreshProjects, refreshPresets, refreshTools]);

  const setViewedPresetId = useCallback((id: string) => {
    setViewedPresetIdState(id);
    try {
      localStorage.setItem(VIEWED_PRESET_LS_KEY, id);
    } catch {
      // localStorage may be unavailable; selection is still tracked in memory.
    }
  }, []);

  // Resolve viewedPreset: persisted id > activePreset > first preset.
  // Persist whichever resolves so the next launch matches what the user saw.
  const viewedPreset = (() => {
    if (viewedPresetId) {
      const found = presets.find((s) => s.id === viewedPresetId);
      if (found) return found;
    }
    return activePreset ?? presets[0] ?? null;
  })();

  useEffect(() => {
    if (!viewedPreset) return;
    if (viewedPreset.id !== viewedPresetId) {
      // Persist the resolved fallback without synchronously changing state.
      // The derived value already keeps the UI stable for this render.
      try {
        localStorage.setItem(VIEWED_PRESET_LS_KEY, viewedPreset.id);
      } catch {
        // ignore
      }
    }
  }, [viewedPreset, viewedPresetId]);

  useEffect(() => {
    async function init() {
      // Both events log performance.now() (ms since timeOrigin) so the
      // reader can compute duration as done - start. Keeping the unit
      // identical to the other frontend startup marks avoids ambiguity in
      // the log file (see codex review note on #153).
      api.logStartupEvent("refresh_app_data_start", performance.now()).catch(() => {});
      await refreshAppData();
      api.logStartupEvent("refresh_app_data_done", performance.now()).catch(() => {});
      // Apply saved text size on startup
      const savedSize = await api.getSettings("text_size").catch(() => null);
      if (savedSize) {
        applyTextSize(savedSize);
      }
    }
    init();
  }, [refreshAppData]);

  return (
    <AppContext.Provider
      value={{
        presets,
        activePreset,
        viewedPreset,
        tools,
        managedSkills,
        projects,
        loading,
        appError,
        helpOpen,
        detailSkillId,
        refreshAppData,
        refreshPresets,
        refreshTools,
        refreshManagedSkills,
        refreshProjects,
        setViewedPresetId,
        clearAppError: () => setAppError(null),
        openHelp: () => setHelpOpen(true),
        closeHelp: () => setHelpOpen(false),
        openSkillDetailById: (skillId: string) => setDetailSkillId(skillId),
        closeSkillDetail: () => setDetailSkillId(null),
      }}
    >
      {children}
    </AppContext.Provider>
  );
}

export function useApp() {
  const ctx = useContext(AppContext);
  if (!ctx) throw new Error("useApp must be used within AppProvider");
  return ctx;
}
