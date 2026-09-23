import type { ManagedSkill } from "./tauri";

export type PickerStatus = "available" | "installed" | "conflict" | "unavailable";

export interface ProjectPickerContext {
  kind: "project";
  /** All skill dir names already present under `<repo>/.agents/skills`. */
  projectSkillDirNames: string[];
  /** Center skill ids already installed under the canonical project root. */
  projectCenterSkillIds: string[];
  dirNameMap: Record<string, string>;
  dirNameMapError: boolean;
}

export type PickerContext = ProjectPickerContext;

export function classifySkill(skill: ManagedSkill, ctx: PickerContext): PickerStatus {
  if (ctx.projectCenterSkillIds.includes(skill.id)) return "installed";

  const dirName = ctx.dirNameMap[skill.id]?.toLowerCase();
  if (ctx.dirNameMapError && !dirName) return "conflict";

  if (dirName && ctx.projectSkillDirNames.includes(dirName)) return "conflict";

  return "available";
}

/** Whether the skill can be installed into the canonical project root. */
export function canInstallToProject(skill: ManagedSkill, ctx: ProjectPickerContext): boolean {
  return classifySkill(skill, ctx) === "available";
}
