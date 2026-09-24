<p align="center">
  <img src="assets/icon.png" width="80" />
</p>

<h1 align="center">AgentDock</h1>

<p align="center">
  统一管理 Agent 的 Skills 与 MCP；只读观察 Harnesses。
</p>

<p align="center">
  <strong><a href="https://skillsmanager.dev/zh">skillsmanager.dev</a></strong>
</p>

<p align="center">
  <small>兼容说明：现有元数据仍位于 <code>~/.skills-manager</code>；过渡期间保留旧的 <code>skills-manager-cli</code> 可执行文件作为别名。GitHub Release 源现在为 <code>Zestful-ss/agentdock</code>；官网和 Homebrew cask 需要在外部发布 Gate 中同步更新。</small>
</p>

<p align="center">
  🎬 <a href="https://www.bilibili.com/video/BV1845F6REUu/">视频介绍（Bilibili）</a>
  &nbsp;·&nbsp;
  <a href="https://www.youtube.com/watch?v=wfbCrfNASVU">Video intro (YouTube)</a>
</p>

<p align="center">
  <a href="./README.md">English</a>
</p>

<p align="center">
  <a href="https://trendshift.io/repositories/23290?utm_source=repository-badge&amp;utm_medium=badge&amp;utm_campaign=badge-repository-23290" target="_blank" rel="noopener noreferrer"><img src="https://trendshift.io/api/badge/repositories/23290" alt="xingkongliang%2Fskills-manager | Trendshift" width="250" height="55"/></a>
</p>

<p align="center">
  <a href="https://skills.sh/Zestful-ss/agentdock"><img src="https://skills.sh/b/Zestful-ss/agentdock" alt="skills.sh 上的 manage-skills" /></a>
</p>

## 功能

- **管理 Agent Skills 与 MCP** —— Skills 只写入用户 `~/.agents/skills/` 和项目 `<repo>/.agents/skills/`；Harness（Claude Code、Codex、Cursor 等）只作为发现来源，在 **Inventory** 中观察，不写入 Harness 专属目录。
- **统一技能库** —— 从 Git 仓库、本地目录、`.zip` / `.skill` 文件安装 Skills。应用元数据（SQLite、缓存、日志）仍位于兼容路径 `~/.skills-manager`，它不是技能库。
- **My Skills** —— 浏览和整理技能库，管理标签与 Preset 成员关系。Preset 是元数据整理，不会部署或删除 Harness 文件。
- **Inventory** —— 只读查看每个 Harness 实际发现的 Skills 与 MCP，包括原生 `.agents` 消费者。
- **Project Workspaces** —— 管理 `<repo>/.agents/skills` 中的项目 Skills，并与用户技能库比较、导入或导出。
- **安装与来源** —— 支持本地目录、Git、压缩包和 skills.sh CLI 安装；保留来源、revision、标签和更新状态。
- **更新与重导入** —— 对 Git Skills 检查远端更新；本地修改会触发保护，不会静默覆盖。
- **发现配置** —— Agent 开关、路径编辑和自定义工具只改变 Inventory 的发现配置，不执行 Harness 写入。
- **备份命令** —— Git remote helpers 仍可从 **设置** 和首次运行恢复流程使用；当前版本没有侧边栏 Backup 页面。
- **活动日志与导出** —— 记录安装、删除、更新和纳管操作；在 **设置 → 导出日志** 打包诊断信息。
- **应用内更新** —— 检查更新只负责通知；安装和重启都需要用户明确点击。

> 文档中的截图待按当前 V1.1 界面重新录制；行为说明以本文和 `architecture/ARCHITECTURE.md` 为准。

## 安装

### macOS

使用 [Homebrew](https://brew.sh) 安装：

```bash
brew install --cask skills-manager
```

也可以从[最新 Release](https://github.com/Zestful-ss/agentdock/releases/latest)下载 `.dmg`。

### Windows 和 Linux

从[最新 Release](https://github.com/Zestful-ss/agentdock/releases/latest)下载对应平台的安装包：Windows 为 `.exe` 或 `.msi`，Linux 为 `.AppImage`、`.deb` 或 `.rpm`（提供 x64 和 arm64）。

所有安装包都自带 CLI，位置见[二进制放在哪](#二进制放在哪)。

## 快速上手

1. 从 **Install** 安装 Skills；文件会进入用户或项目的 canonical `.agents/skills` 目录。
2. 打开 **My Skills** 整理技能库、标签和 Preset 成员关系。
3. 打开 **Inventory** 查看 Harness 实际发现的 Skills 与 MCP；这里是只读观察面。
4. 如需管理项目 Skills，打开 **Project** 并选择 `<repo>/.agents/skills`。
5. 在 **Settings** 配置发现路径、主题、语言、Git remote 和更新检查。

## 让你的 Agent 管理 Skills

Claude Code、Codex、Cursor 等可以通过 [`manage-skills`](skills/manage-skills/SKILL.md) skill 和 `agentdock-cli` 管理 canonical 技能库。Agent 不会绕过应用直接写入 Harness 目录；来源元数据、Preset 归属和更新状态因此保持一致。

应用启动时会发布一份与桌面端版本一致的 CLI。旧 Agent 仍可使用 `skills-manager-cli` 兼容名称；新安装优先使用 `agentdock-cli`。

CLI 也可以独立安装：

```bash
npx skills add Zestful-ss/agentdock
```

## Git 备份与多设备同步

Git backup helpers 仍然保留，但入口位于 **Settings → Git Sync Configuration** 和首次运行恢复对话框，而不是侧边栏页面。

- **连接** —— 支持 GitHub 登录、任意 HTTPS + PAT、SSH 或自建 Git 服务。令牌只保存在系统钥匙串。
- **同步** —— 本地改动会在空闲后提交并推送；远端更新会在仓库锁内合并。
- **冲突** —— 同一 Skill 的并发修改会保留本机版本并进入待处理状态；应用不会静默覆盖用户内容。
- **快照** —— 备份历史可恢复；恢复前会先保留当前状态。
- **隐私** —— API Key、令牌、代理配置和本机路径不会进入远端仓库。

## V1 发现来源

当前架构的发现 allowlist 包含 Codex、Claude Code、OpenCode、DeepSeek Harness、Kimi、Grok、Pi、Antigravity、Maka 和 OpenChamber 等来源。Harness 配置只用于观察 Skills 与 MCP，不是部署目标。

## 技术栈

| 层 | 技术 |
|----|------|
| 前端 | React 19、TypeScript、Vite、Tailwind CSS |
| 桌面 | Tauri 2 |
| 后端 | Rust |
| 存储 | SQLite（`rusqlite`） |
| 国际化 | react-i18next |

## 开发

### 前置依赖

- Node.js 20.19+ 或 22.12+（Vite 7 的要求）
- Rust 1.77.2 或更高
- 当前系统的 [Tauri 依赖](https://v2.tauri.app/start/prerequisites/)

```bash
npm install
npm run tauri:dev
```

### CLI

仓库包含一个面向 Agent 的 CLI，与桌面应用共用 Rust core、SQLite 数据库和仓库锁。

```bash
# 查看技能库
npm run cli -- skills list
npm run cli -- skills show db

# 只写入 canonical .agents/skills
npm run cli -- skills install ./my-skill
npm run cli -- skills install https://github.com/foo/bar/tree/main/skills/baz
npm run cli -- skills install vercel-labs/agent-skills@react-best-practices
npm run cli -- skills status react-best-practices

# 更新检查与纳管
npm run cli -- skills check --all
npm run cli -- skills update --all
npm run cli -- skills adopt ~/.claude/skills --dry-run
```

命令组包括：

- `agents`（兼容别名 `tools`）：查看发现来源
- `skills`：管理 canonical 技能库、标签和 Preset 成员关系
- `presets`：创建、修改、删除、整理和查看 Preset
- `git`：操作 Git 备份仓库

破坏性命令支持相应的 `--dry-run`；`remove` 仍要求显式 `--yes`。机器调用建议始终加 `--json`。

### 二进制放在哪

应用启动时会把自己那份 CLI 复制到 `~/.skills-manager/bin/agentdock-cli`，版本永远与正在运行的应用一致，Agent 不需要修改 PATH 就能找到它。旁边的 `.version` 标记只在副本校验通过后写入，并在每次重新发布前删除；旧版 Agent 仍可回退到 `skills-manager-cli` 路径。

```bash
npm run cli:install
# 等价于：
# cargo install --path src-tauri --bin agentdock-cli --bin skills-manager-cli --locked --force
```

这会安装 `~/.cargo/bin/agentdock-cli`，并继续提供旧的 `skills-manager-cli` 名称。

正式 Release 会提供 macOS arm64/x64、Windows x64、Linux x64/arm64 的 `agentdock-cli-*` 独立 CLI 文件；兼容期间也保留旧的 `skills-manager-cli-*` 资产。

### 构建

```bash
npm run tauri:build
npm run cli:build
```

## 常见问题

**macOS 打不开应用。** 从 **v1.29.0** 起，发布版本都经过 Apple Developer ID 签名与公证。若仍看到「应用已损坏」，请升级到最新版本。升级后 macOS 可能再次询问 `skills-manager-git-backup` 钥匙串权限，请选择「始终允许」。

其它问题请[提交 issue](https://github.com/Zestful-ss/agentdock/issues)，并附上 **设置 → 导出日志** 生成的压缩包。

## Star 增长

<p align="center">
  <a href="https://github.com/xingkongliang/star-history-svg">
    <img src="assets/star-history.svg" width="800" alt="Zestful-ss/agentdock 的 Star History 图" />
  </a>
</p>

## License

MIT
