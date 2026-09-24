import { invoke } from "@tauri-apps/api/core";

// ── Types ──

export type ToolCategory = "coding" | "lobster";

export interface ToolInfo {
  key: string;
  display_name: string;
  installed: boolean;
  skills_dir: string;
  enabled: boolean;
  is_custom: boolean;
  has_path_override: boolean;
  project_relative_skills_dir: string | null;
  has_project_path_override: boolean;
  category: ToolCategory;
}

export interface ManagedSkill {
  id: string;
  name: string;
  description: string | null;
  source_type: string;
  source_ref: string | null;
  source_ref_resolved: string | null;
  source_subpath: string | null;
  source_branch: string | null;
  source_revision: string | null;
  remote_revision: string | null;
  update_status: string;
  last_checked_at: number | null;
  last_check_error: string | null;
  central_path: string;
  enabled: boolean;
  created_at: number;
  updated_at: number;
  status: string;
  preset_ids: string[];
  tags: string[];
}

export interface SkillDocument {
  skill_id: string;
  filename: string;
  content: string;
  central_path: string;
}

export interface SourceSkillDocument {
  skill_id: string;
  filename: string;
  content: string;
  source_label: string;
  revision: string;
}

export type SkillSourceDiffStatus = "added" | "removed" | "modified";
export type SkillSourceDiffContentKind =
  | "text"
  | "binary"
  | "too_large"
  | "permission_only";

export interface SkillSourceDiffEntry {
  relative_path: string;
  status: SkillSourceDiffStatus;
  content_kind: SkillSourceDiffContentKind;
  original_text: string | null;
  updated_text: string | null;
  executable_before: boolean;
  executable_after: boolean;
}

export interface SkillSourceDiff {
  skill_id: string;
  source_label: string;
  revision: string;
  entries: SkillSourceDiffEntry[];
}

export interface Preset {
  id: string;
  name: string;
  description: string | null;
  icon: string | null;
  sort_order: number;
  skill_count: number;
  created_at: number;
  updated_at: number;
}

export interface SyncHealth {
  in_sync: number;
  project_newer: number;
  center_newer: number;
  diverged: number;
  project_only: number;
}

export interface Project {
  id: string;
  name: string;
  path: string;
  workspace_type: "project" | "linked";
  linked_agent_name: string | null;
  sort_order: number;
  skill_count: number;
  sync_health: SyncHealth;
  created_at: number;
  updated_at: number;
}

export interface ProjectSkill {
  name: string;
  dir_name: string;
  relative_path: string;
  description: string | null;
  path: string;
  files: string[];
  enabled: boolean;
  agent: string;
  agent_display_name: string;
  tags: string[];
  in_center: boolean;
  sync_status: "project_only" | "in_sync" | "project_newer" | "center_newer" | "diverged";
  center_skill_id: string | null;
}

export interface ProjectSkillDocument {
  skill_name: string;
  filename: string;
  content: string;
}

// ── Tools ──

export const getToolStatus = () => invoke<ToolInfo[]>("get_tool_status");

export const setToolEnabled = (key: string, enabled: boolean) =>
  invoke<void>("set_tool_enabled", { key, enabled });

export const setAllToolsEnabled = (enabled: boolean) =>
  invoke<void>("set_all_tools_enabled", { enabled });

export const getToolOrder = () => invoke<string[]>("get_tool_order_cmd");

export const setToolOrder = (order: string[]) =>
  invoke<void>("set_tool_order_cmd", { order });

export const setCustomToolPath = (key: string, path: string) =>
  invoke<void>("set_custom_tool_path", { key, path });

export const resetCustomToolPath = (key: string) =>
  invoke<void>("reset_custom_tool_path", { key });

export const setCustomToolProjectPath = (
  key: string,
  projectRelativeSkillsDir: string | null,
) =>
  invoke<void>("set_custom_tool_project_path", {
    key,
    projectRelativeSkillsDir,
  });

export const resetCustomToolProjectPath = (key: string) =>
  invoke<void>("reset_custom_tool_project_path", { key });

export const addCustomTool = (
  key: string,
  displayName: string,
  skillsDir: string,
  projectRelativeSkillsDir?: string,
) =>
  invoke<void>("add_custom_tool", {
    key,
    displayName,
    skillsDir,
    projectRelativeSkillsDir: projectRelativeSkillsDir ?? null,
  });

export const removeCustomTool = (key: string) =>
  invoke<void>("remove_custom_tool", { key });

// ── Skills ──

export const getManagedSkills = () =>
  invoke<ManagedSkill[]>("get_managed_skills");

export const getSkillsForPreset = (presetId: string) =>
  invoke<ManagedSkill[]>("get_skills_for_preset", {
    presetId,
  });

export const getSkillDocument = (skillId: string) =>
  invoke<SkillDocument>("get_skill_document", { skillId });

export const getSourceSkillDocument = (skillId: string) =>
  invoke<SourceSkillDocument>("get_source_skill_document", { skillId });

export const getSkillSourceDiff = (skillId: string) =>
  invoke<SkillSourceDiff>("get_skill_source_diff", { skillId });

export const deleteManagedSkill = (skillId: string) =>
  invoke<void>("delete_managed_skill", { skillId });

export interface BatchDeleteSkillsResult {
  deleted: number;
  failed: string[];
}

export const deleteManagedSkills = (skillIds: string[]) =>
  invoke<BatchDeleteSkillsResult>("delete_managed_skills", { skillIds });

export const installLocal = (sourcePath: string, name?: string) =>
  invoke<void>("install_local", { sourcePath, name: name || null });

export interface GitSkillPreview {
  /** Path relative to the resolved scan root, using `/` separators. Stable key. */
  rel_path: string;
  name: string;
  description: string | null;
}

export interface GitPreviewResult {
  temp_dir: string;
  skills: GitSkillPreview[];
}

export interface SkillInstallItem {
  rel_path: string;
  name: string;
}

export type GitInstallScope = "user" | "project";

/** Per-item result of a git install batch. */
export interface GitInstallOutcome {
  rel_path: string;
  name: string;
  status: "installed" | "conflict" | "failed";
  dest_path: string | null;
  error: string | null;
}

/** Batch result: outcomes plus temp lifecycle for conflict retries. */
export interface GitConfirmResult {
  outcomes: GitInstallOutcome[];
  /** True while a conflict retry may still need the clone. */
  temp_retained: boolean;
}

export const previewGitInstall = (repoUrl: string) =>
  invoke<GitPreviewResult>("preview_git_install", { repoUrl });

export const confirmGitInstall = (
  repoUrl: string,
  tempDir: string,
  items: SkillInstallItem[],
  scope?: GitInstallScope | null,
  projectId?: string | null,
  replace?: boolean
) =>
  invoke<GitConfirmResult>("confirm_git_install", {
    repoUrl,
    tempDir,
    items,
    scope: scope ?? null,
    projectId: projectId ?? null,
    replace: replace ?? null,
  });

export const cancelGitPreview = (tempDir: string) =>
  invoke<void>("cancel_git_preview", { tempDir });

export const cancelInstall = (key: string) =>
  invoke<boolean>("cancel_install", { key });

export const checkSkillUpdate = (skillId: string, force?: boolean) =>
  invoke<ManagedSkill>("check_skill_update", {
    skillId,
    force: force ?? false,
  });

export const checkAllSkillUpdates = (force?: boolean) =>
  invoke<void>("check_all_skill_updates", {
    force: force ?? false,
  });

export interface UpdateSkillResult {
  skill: ManagedSkill;
  /** False when a monorepo commit didn't touch this skill's subdirectory. */
  content_changed: boolean;
  /**
   * What the update would remove. Non-empty means **nothing was changed** —
   * show these and call again with `removal_approval` if the user accepts.
   */
  pending_removals: PendingRemoval[];
  /**
   * Identifies exactly what `pending_removals` describes. Passing it back
   * approves that list at that revision and nothing else.
   */
  removal_approval: string | null;
}

export interface PendingRemoval {
  /** `"library"`, or the agent key whose deployed copy holds it. */
  location: string;
  path: string;
}

/** `approvedRemovals` carries back `removal_approval` from a declined call. */
export const updateSkill = (skillId: string, approvedRemovals?: string | null) =>
  invoke<UpdateSkillResult>("update_skill", {
    skillId,
    approvedRemovals: approvedRemovals ?? null,
  });

export interface BatchUpdateSkillsResult {
  refreshed: number;
  unchanged: number;
  /** Skills left alone because updating would have removed files. */
  held_back: string[];
  failed: string[];
}

export const batchUpdateSkills = (skillIds: string[]) =>
  invoke<BatchUpdateSkillsResult>("batch_update_skills", { skillIds });

export interface ReimportSkillResult {
  skill: ManagedSkill;
  /** Non-empty means nothing was changed — see UpdateSkillResult. */
  pending_removals: PendingRemoval[];
  /** Approves exactly `pending_removals` — see UpdateSkillResult. */
  removal_approval: string | null;
}

export const reimportLocalSkill = (skillId: string, approvedRemovals?: string | null) =>
  invoke<ReimportSkillResult>("reimport_local_skill", {
    skillId,
    approvedRemovals: approvedRemovals ?? null,
  });

export const relinkLocalSkillSource = (
  skillId: string,
  sourcePath: string,
  approvedRemovals?: string | null,
) =>
  invoke<ReimportSkillResult>("relink_local_skill_source", {
    skillId,
    sourcePath,
    approvedRemovals: approvedRemovals ?? null,
  });

export const detachLocalSkillSource = (skillId: string) =>
  invoke<ManagedSkill>("detach_local_skill_source", { skillId });

export const getAllTags = () => invoke<string[]>("get_all_tags");

export const setSkillTags = (skillId: string, tags: string[]) =>
  invoke<void>("set_skill_tags", { skillId, tags });

export const renameTag = (oldName: string, newName: string) =>
  invoke<void>("rename_tag", { oldName, newName });

export const deleteTag = (name: string) =>
  invoke<void>("delete_tag", { name });

// ── Settings ──

export const getSettings = (key: string) =>
  invoke<string | null>("get_settings", { key });

export const setSettings = (key: string, value: string) =>
  invoke<void>("set_settings", { key, value });

export const getCentralRepoPath = () =>
  invoke<string>("get_central_repo_path");

export const getCentralRepoPathOverride = () =>
  invoke<string | null>("get_central_repo_path_override");

export const getCentralRepoWarnings = () =>
  invoke<string[]>("get_central_repo_warnings");

export const setCentralRepoPath = (path?: string | null) =>
  invoke<string>("set_central_repo_path", { path: path ?? null });

export const appExit = () => invoke<void>("app_exit");

export const openCentralRepoFolder = () =>
  invoke<void>("open_central_repo_folder");

export interface DiagnosticInfo {
  app_version: string;
  os: string;
  os_version: string;
  arch: string;
  central_repo_path: string;
  central_repo_path_overridden: boolean;
}

export const getDiagnosticInfo = () =>
  invoke<DiagnosticInfo>("get_diagnostic_info");

export interface LogExcerpt {
  log_path: string;
  excerpt: string;
  line_count: number;
  has_warnings: boolean;
}

export const getRecentLogExcerpt = () =>
  invoke<LogExcerpt>("get_recent_log_excerpt");

export interface LogExportResult {
  zip_path: string;
  file_count: number;
}

export const exportLogsZip = () =>
  invoke<LogExportResult>("export_logs_zip");

export interface PanicInfo {
  timestamp: string;
  message: string;
}

export const checkLastPanic = () =>
  invoke<PanicInfo | null>("check_last_panic");

export const clearLastPanic = () =>
  invoke<void>("clear_last_panic");

/**
 * Diagnostic-only: write a named startup event with elapsed ms (from
 * performance.timeOrigin) into the backend log file. Used to correlate
 * WebView2 boot and frontend boot timing with Rust-side startup logs
 * when debugging slow launches (see issue #153).
 */
export const logStartupEvent = (label: string, elapsedMs: number) =>
  invoke<void>("log_startup_event", { label, elapsedMs: Math.round(elapsedMs) });

// ── Presets ──

export const getPresets = () => invoke<Preset[]>("get_presets");

export const getActivePreset = () =>
  invoke<Preset | null>("get_active_preset");

export const createPreset = (name: string, description?: string, icon?: string) =>
  invoke<Preset>("create_preset", {
    name,
    description: description || null,
    icon: icon || null,
  });

export const updatePreset = (
  id: string,
  name: string,
  description?: string,
  icon?: string
) =>
  invoke<void>("update_preset", {
    id,
    name,
    description: description || null,
    icon: icon || null,
  });

export const deletePreset = (id: string) =>
  invoke<void>("delete_preset", { id });

export const addSkillToPreset = (skillId: string, presetId: string) =>
  invoke<void>("add_skill_to_preset", { skillId, presetId });

export const removeSkillFromPreset = (skillId: string, presetId: string) =>
  invoke<void>("remove_skill_from_preset", { skillId, presetId });

export const reorderPresets = (ids: string[]) =>
  invoke<void>("reorder_presets", { ids });

export const reorderProjects = (ids: string[]) =>
  invoke<void>("reorder_projects", { ids });

export const getPresetSkillOrder = (presetId: string) =>
  invoke<string[]>("get_preset_skill_order", { presetId });

export const reorderPresetSkills = (presetId: string, skillIds: string[]) =>
  invoke<void>("reorder_preset_skills", { presetId, skillIds });

// ── Projects ──

export const getProjects = () => invoke<Project[]>("get_projects");

export const addProject = (path: string) =>
  invoke<Project>("add_project", { path });

export const removeProject = (id: string) =>
  invoke<void>("remove_project", { id });

export const scanProjects = (root: string) =>
  invoke<string[]>("scan_projects", { root });

export const getProjectSkills = (projectId: string) =>
  invoke<ProjectSkill[]>("get_project_skills", { projectId });

export const getProjectSkillDocument = (projectId: string, skillRelativePath: string) =>
  invoke<ProjectSkillDocument>("get_project_skill_document", { projectId, skillRelativePath });

export const importProjectSkillToCenter = (projectId: string, skillRelativePath: string) =>
  invoke<void>("import_project_skill_to_center", { projectId, skillRelativePath });

export const copySkillToProject = (skillId: string, projectId: string) =>
  invoke<void>("copy_skill_to_project", { skillId, projectId });

export const updateProjectSkillToCenter = (projectId: string, skillRelativePath: string) =>
  invoke<void>("update_project_skill_to_center", { projectId, skillRelativePath });

export const updateProjectSkillFromCenter = (projectId: string, skillRelativePath: string) =>
  invoke<void>("update_project_skill_from_center", { projectId, skillRelativePath });

export const deleteProjectSkill = (projectId: string, skillRelativePath: string) =>
  invoke<void>("delete_project_skill", { projectId, skillRelativePath });

export const slugifySkillNames = (names: string[]) =>
  invoke<string[]>("slugify_skill_names", { names });

// ── V1 Inventory ──

export type SkillLifecycle =
  | "managed"
  | "discovered"
  | "read_only"
  | "system"
  | "conflict"
  | "update_available";

export interface SkillInventoryRow {
  name: string;
  status: SkillLifecycle;
  /** canonical, harness, or custom. */
  source_kind: "canonical" | "harness" | "custom";
  /** agentdock, external, or system. */
  ownership: "agentdock" | "external" | "system";
  path: string;
  source_harness: string;
  source_display_name: string;
  description: string | null;
  fingerprint: string | null;
  system: boolean;
  read_only: boolean;
  native_consumers: string[];
}

export type McpTransport = "stdio" | "streamable_http" | "legacy_sse" | "unknown";

export interface McpHarnessStatus {
  harness: string;
  display_name: string;
  /** Transport observed in this harness's own config. Never sampled. */
  transport: McpTransport;
  /** true/false when stated; null means "Configured" (unknown), never "Enabled". */
  source_enabled: boolean | null;
  source_path: string;
  configured: boolean;
}

/** One observed MCP resource. Duplicate names from different Harnesses remain
 * separate rows so their source configs are never merged. */
export interface McpInventoryRow {
  id: string;
  name: string;
  sources: McpHarnessStatus[];
}

export interface CanonicalRoots {
  user_skills: string;
  native_consumers: string[];
}

export type CanonicalScope = "user" | "project";

export interface SkillDocument {
  skill_name: string;
  filename: string;
  content: string;
  path: string;
}

export interface MigrationEntry {
  name: string;
  legacy_path: string;
  canonical_path: string;
  outcome: "migrated" | "updated_db_only" | "conflict" | "adopted" | "failed";
  content_hash: string | null;
  /** Set only for failed rows. */
  error: string | null;
}

export const getSkillInventory = () =>
  invoke<SkillInventoryRow[]>("get_skill_inventory");

export const getCustomReadOnlyPaths = () =>
  invoke<string[]>("get_custom_read_only_paths");

export const setCustomReadOnlyPaths = (paths: string[]) =>
  invoke<string[]>("set_custom_read_only_paths", { paths });

export const getProjectSkillInventory = (projectId: string) =>
  invoke<SkillInventoryRow[]>("get_project_skill_inventory", { projectId });

export const getMcpInventory = () =>
  invoke<McpInventoryRow[]>("get_mcp_inventory");

export const adoptSkillToUser = (sourcePath: string, replace?: boolean) =>
  invoke<string>("adopt_skill_to_user", { sourcePath, replace: replace ?? null });

export const adoptSkillToProject = (
  sourcePath: string,
  projectId: string,
  replace?: boolean
) =>
  invoke<string>("adopt_skill_to_project", { sourcePath, projectId, replace: replace ?? null });

export const deleteCanonicalSkill = (
  skillName: string,
  scope: CanonicalScope,
  projectId?: string | null
) =>
  invoke<string>("delete_canonical_skill", {
    skillName,
    scope,
    projectId: projectId ?? null,
  });

export const readCanonicalSkillDocument = (
  skillName: string,
  scope: CanonicalScope,
  projectId?: string | null
) =>
  invoke<SkillDocument>("read_canonical_skill_document", {
    skillName,
    scope,
    projectId: projectId ?? null,
  });

export const saveCanonicalSkillDocument = (
  skillName: string,
  content: string,
  scope: CanonicalScope,
  projectId?: string | null
) =>
  invoke<string>("save_canonical_skill_document", {
    skillName,
    content,
    scope,
    projectId: projectId ?? null,
  });

export const runLegacyMigration = () =>
  invoke<MigrationEntry[]>("run_legacy_migration");

export const getCanonicalRoots = () =>
  invoke<CanonicalRoots>("get_canonical_roots");
