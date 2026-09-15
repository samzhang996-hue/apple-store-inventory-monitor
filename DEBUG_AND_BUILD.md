# 果到雷达（Apple Store Inventory Monitor）调试与打包指南

本项目基于 **Tauri 2 + React 19 + Vite + TailwindCSS 4 + Rust** 架构开发。以下为完整的本地环境准备、调试运行、代码校验与打包发布指南。

---

## 目录
1. [环境准备](#一环境准备)
2. [本地调试（Debug）](#二本地调试debug)
3. [编译校验（打包前验证）](#三编译校验打包前验证)
4. [应用打包（Build / Release）](#四应用打包build--release)
5. [常见问题排查（FAQ）](#五常见问题排查faq)

---

## 一、环境准备

在终端中执行命令前，请确保安装并配置好以下基础工具链：

| 工具 | 推荐版本 | 检查命令 | 说明 |
| :--- | :--- | :--- | :--- |
| **Node.js** | 20.19+ 或 22.12+ (LTS) | `node -v` | JavaScript 运行时 |
| **pnpm** | 9+ | `pnpm -v` | 包管理器（若未安装：`npm install -g pnpm`） |
| **Rust / Cargo** | 1.90+ | `cargo --version` | 后端核心引擎（若未安装见 [rustup.rs](https://rustup.rs)） |
| **Xcode CLT** | 最新版本 | `xcode-select -p` | macOS 原生编译工具（未安装执行 `xcode-select --install`） |

> **提示**：若终端提示找不到 `cargo`，请先运行：
> ```bash
> source "$HOME/.cargo/env"
> ```

---

## 二、本地调试（Debug）

进入项目根目录：
```bash
cd /Users/macmini/Documents/apple-store-inventory-monitor
```

### 方式 1：完整桌面客户端调试（推荐）
同时拉起 Rust 原生后端与前端 Vite 热重载，提供与正式版完全一致的桌面窗口、系统托盘以及真实库存轮询监控能力：

```bash
# 1. 安装依赖（首次运行或依赖有变动时）
pnpm install

# 2. 加载 Cargo 环境变量
source "$HOME/.cargo/env"

# 3. 启动桌面端开发模式
pnpm tauri dev
```

#### 桌面端调试技巧：
- **开发者工具（DevTools）**：桌面窗口启动后，按快捷键 `Cmd + Option + I` 或在窗口空白处点击右键选择「检查（Inspect）」，即可唤出 Chrome/WebKit 审查元素与 Console 控制台。
- **前端热更新（HMR）**：修改 `src/` 目录下的 React 组件（如 `src/App.tsx`），页面会自动无缝更新，无需重启客户端。
- **后端热编译**：修改 `src-tauri/` 或 `crates/` 目录下的 Rust 核心代码时，Tauri 会自动重新编译 Rust 并重载窗口。

---

### 方式 2：纯前端网页调试（仅界面排版）
如果仅需快速调整页面样式、Tailwind 样式或组件排版，不需要与 Rust 原生底层通信：

```bash
pnpm dev
```
- 启动后访问浏览器地址：`http://localhost:5173`
- **注意**：纯网页模式下无法调用 Tauri Rust 命令（如系统通知、真实 Chromium 查询会话），仅用于快速预览 UI。

---

## 三、编译校验（打包前验证）

在准备打包或提交代码前，建议运行以下命令进行代码规范、TypeScript 类型检查和测试套件验证：

```bash
# 1. 前端类型检查与 Vite 生产构建测试
pnpm build

# 2. 前端单元测试
pnpm test

# 3. Rust 代码格式与静态分析
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings

# 4. Rust 离线单元测试
cargo test --workspace
```

---

## 四、应用打包（Build / Release）

### 本地打包生成安装包

由于本地构建通常不需要官方私钥自动更新签名，使用 `--no-sign` 参数进行无签名打包：

```bash
cd /Users/macmini/Documents/apple-store-inventory-monitor

# 确保环境变量正常
source "$HOME/.cargo/env"

# 执行 Tauri 打包
pnpm tauri build --no-sign
```

### 打包输出产物路径

构建成功后，安装文件位于 `target/release/bundle/` 目录下：

- **macOS DMG 镜像**：
  `target/release/bundle/dmg/Apple Store Inventory Monitor_1.0.6_aarch64.dmg`（Apple Silicon）或 `_x64.dmg`（Intel）
- **macOS .app 应用程序**：
  `target/release/bundle/macos/Apple Store Inventory Monitor.app`

---

## 五、常见问题排查（FAQ）

### 1. 启动或打包时报 `failed to run cargo metadata`
- **原因**：当前终端环境中没有找到 `cargo` 命令。
- **解决**：
  ```bash
  source "$HOME/.cargo/env"
  # 验证是否生效
  cargo --version
  ```

### 2. 目录重命名或迁移后，构建日志报旧目录路径错误
- **原因**：Cargo 在 `target/` 目录中保留了原目录的绝对路径缓存。
- **解决**：在项目根目录运行清理命令后再打包：
  ```bash
  cargo clean
  pnpm tauri build --no-sign
  ```

### 3. 打开打包好的 `.dmg` 或 `.app` 提示「应用无法验证开发者」或「已损坏」
- **原因**：本地构建的应用采用 ad-hoc 临时签名，未经过 Apple 官方公证（Notarization）。
- **解决**：
  在终端中执行以下命令解除 macOS 隔离标记：
  ```bash
  xattr -dr com.apple.quarantine "/Applications/Apple Store Inventory Monitor.app"
  ```
  或者在 Finder 中按住 `Control` 键点击该应用，选择「打开」。
