import { useCallback, useMemo, useState } from "react";
import { useApp } from "../context/AppContext";

export const CURRENT_PROJECT_LS_KEY = "skills-manager.currentProjectId";

/**
 * The single-project context shared by Inventory and Install.
 * Identity is explicit: a stale persisted id resolves to null (never to
 * "whatever happens to be first"). The only implicit case is a single
 * linked project — the one possible target — which needs no persistence.
 */
export function useCurrentProject() {
  const { projects } = useApp();
  const [currentProjectId, setCurrentProjectId] = useState<string | null>(() => {
    try {
      return localStorage.getItem(CURRENT_PROJECT_LS_KEY);
    } catch {
      return null;
    }
  });

  const selectProject = useCallback((id: string | null) => {
    setCurrentProjectId(id);
    try {
      if (id) localStorage.setItem(CURRENT_PROJECT_LS_KEY, id);
      else localStorage.removeItem(CURRENT_PROJECT_LS_KEY);
    } catch {
      // localStorage may be unavailable; selection is still tracked in memory.
    }
  }, []);

  const currentProject = useMemo(() => {
    if (projects.length === 0) return null;
    if (currentProjectId) {
      return projects.find((p) => p.id === currentProjectId) ?? null;
    }
    if (projects.length === 1) return projects[0];
    return null;
  }, [projects, currentProjectId]);

  return { projects, currentProject, currentProjectId, selectProject };
}
