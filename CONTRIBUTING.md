# 贡献指南

本仓库采用 **Git Flow** 分支模型，所有分支命名使用 **kebab-case**，发布 tag 使用
SSH 签名。提交前置检查、测试与构建命令见 [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)。

## 分支模型

两条常驻主干分支：

| 分支 | 角色 |
| --- | --- |
| `master`（远端即 `origin/master`） | 生产稳定发布线，只接受 `release/*` / `hotfix/*` 收敛合入，**不直推** |
| `develop` | 最新集成开发状态，汇集所有已完成的 `feature/*` / `fix/*` |

短期分支一律从 `develop` 开出、做完合回 `develop`（`hotfix/*` 例外，从 `master` 开出）：

| 分支 | 从哪出 | 合到哪 | 用途 |
| --- | --- | --- | --- |
| `feature/*` | `develop` | `develop` | 新功能 |
| `fix/*` | `develop` | `develop` | 普通缺陷修复 |
| `release/*` | `develop` | `master` + `develop` | 发布收版、版本号收敛 |
| `hotfix/*` | `master` | `master` + `develop` | 主干紧急修复，必须回灌 develop 防回归 |

流向示意：

```text
feature/* ──► develop ──► release/* ──► master（生产）
fix/* ────► develop ┘                 ▲
hotfix/* ─────────────────────────────┘
```

合流规则：

- `feature/*` / `fix/*` 做完通过 PR 合回 `develop`，通常快进合并即可。
- `release/*` / `hotfix/*` 必须**双向合流**：先以 `git merge --no-ff` 合入 `master`，
  再回灌 `develop`。`--no-ff` 保留合并提交，让发布点在历史上可追溯；漏掉回灌会让
  `develop` 与 `master` 分叉，下个发布把已修好的缺陷再带回 `master`。
- 发布点从 `release/*`（或 `hotfix/*`）分支上打**带签名的 tag**，tag 与该分支版本号同源
  （`release/vX.Y.Z` ↔ `vX.Y.Z`）；合流完成、divergence 为 `0/0` 后才推送 tag。
- 合流完成后删除源分支，避免分支堆积。

## 命名规范

- 所有分支名使用 **kebab-case**（全小写、单词间用连字符 `-` 分隔，不用下划线或大写）：
  `feature/runtime-auto-update`、`fix/window-title-sync`、`release/v1.2.0`、
  `hotfix/fix-token-auth`。
- 发布 tag 用语义化版本 `vMAJOR.MINOR.PATCH`（如 `v1.2.0`），与 `release/*` 分支名后缀一致。
- 提交信息格式：`type(scope): 中文描述`，`type` 取 `feat` / `fix` / `perf` /
  `refactor` / `chore` / `docs`。

## 签名约定

发布 tag 用 **SSH 签名**（不是 GPG 密钥）。本机参考配置来自兄弟仓库
`dsh-smart-provider` 的 `.git/config`，本仓库按同一口径设置：

```ini
[gpg]
	format = ssh
[user]
	signingkey = /home/pt/.ssh/id_ed25519_github_docker_panel.pub
[tag]
	gpgSign = true
[gpg "ssh"]
	allowedSignersFile = /home/pt/Work/DshProjects/dsh-plugin/dsh-tauri/.git/allowed_signers
```

- 签名公钥：`/home/pt/.ssh/id_ed25519_github_docker_panel.pub`（ed25519）。
- `gpg.format=ssh`：用 SSH 密钥做签名。
- `tag.gpgSign=true`：`git tag` 默认带签名；发布时也可用 `git tag -s vX.Y.Z -m "发布描述"` 显式签名。
- `gpg.ssh.allowedSignersFile`：本地验签用的 `allowed_signers` 文件，每行格式为
  `<身份> namespaces="git" <公钥类型与内容> <注释>`，例如：

  ```text
  pengtang1024@qq.com namespaces="git" ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMnIKIxtCye9OdSzDcP9Yxmoh5E8ODdH7lVY8OuNYYEQ docker-panel
  ```

  该文件放在本仓库 `.git/allowed_signers`（随 `.git` 目录本地保存，不进版本库）。

## 边界与禁忌

- **不修改 GitHub 默认分支设置**：默认分支由仓库管理员在平台侧维护，本文所述分支流向
  不要求也不得改动该设置。
- **禁止 force push**：已推送历史只增不改，不对 `master` / `develop` 强推。
- **禁止 rebase**（含全部变体）：合流一律用 `git merge`。
- **不直推 `master`**：稳定版本只经 `release/*` / `hotfix/*` 合入。
- 提交前跑通 [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) 的验证命令（探针、`npm test`、
  `cargo test` / `cargo build`）。
