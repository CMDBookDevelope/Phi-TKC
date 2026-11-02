# 移动端（iOS / iPadOS / Android）支持说明

Phi-TKC 现在会通过 GitHub Actions 打包移动端产物（见 `.github/workflows/mobile.yml`）：

| 平台 | 产物 | 签名 | 说明 |
| --- | --- | --- | --- |
| iOS / iPadOS | `Phi-TKC.ipa` | **未签名** | 需要自行侧载 / 用 AltStore、Sideloadly 等重签；`gen/apple` 工程由 CI 现场 `tauri ios init` 生成 |
| Android | `app-universal-release-unsigned.apk` | **未签名** | 安装前需自行签名（`apksigner`），或改用它重新打包 |

`gen/android`、`gen/apple` 不入库：本机没有 macOS/Xcode 与 Android SDK，无法在本地生成；
CI 每次运行都会 `tauri {android,ios} init --ci` 重新生成。

## 为什么移动端功能不完整

核心渲染管线是**桌面架构**：

1. `src-tauri/src/task.rs` 渲染时会 **spawn 自身二进制**（`current_exe render ...`）作为子进程；
2. 该子进程再调用**外部 ffmpeg 可执行文件**编码（`render.rs`），并把帧通过管道喂过去；
3. 硬件加速探测依赖 `nvidia-smi` / `vainfo` / `system_profiler` 等**外部命令**。

iOS **禁止应用 spawn 子进程**，Android 上也没有可用的 ffmpeg 可执行文件，因此这套管线在移动端
无法工作。移动端版本因此被定位为「能装、能开、可配置」的伴生版本，渲染相关入口会被替换为明确
的说明页（`src/components/MobileUnsupported.vue`）。

## 功能支持矩阵

| 功能 | 桌面端 | 移动端 | 说明 |
| --- | --- | --- | --- |
| 渲染谱面（RenderView） | ✅ | ❌ 屏蔽 | 需要子进程 + 外部 ffmpeg |
| 批量渲染（BatchRenderView） | ✅ | ❌ 屏蔽 | 同上 |
| RPE 绑定（RPEView） | ✅ | ❌ 屏蔽 | 需要选目录 + 读任意路径 |
| 预览（Play/预览窗口） | ✅ | ❌ 屏蔽 | 独立 macroquad 窗口，移动端没有等价物 |
| 任务列表（TasksView） | ✅ | ✅ 可用 | 移动端基本为空；「在文件夹中显示」已隐藏 |
| 设置：主题色 / 自定义背景 / 语言 | ✅ | ✅ 可用 | 语言支持 English / 简体中文，切换后重载；背景写入应用沙箱（AppData） |
| 设置：输出目录 | ✅ | ❌ 隐藏 | 移动端文件选择器不支持选目录；输出固定为应用数据目录 |
| 关于（AboutView） | ✅ | ✅ 可用 | 彩蛋动画在移动端会消耗电量，已降低模糊开销 |
| 拖拽谱面文件 | ✅ | ❌ 无 | 触屏没有拖拽来源；改用文件选择器 |
| 右键菜单 | ✅ | ⚠️ 替代 | 任务卡新增「⋮」按钮，触屏可及 |
| 「在文件夹中显示」/ 打开输出目录 | ✅ | ❌ 隐藏 | `explorer` / `gdbus` / `open` 是桌面 shell 集成 |
| 多实例锁（`app.lock`） | ✅ | ⏭ 跳过 | 移动端恒被视为唯一实例 |
| 硬件加速（NVENC/QSV/AMF/Vulkan） | ✅ | ❌ | 移动端探测一律返回 false |

## 移动端做了哪些适配

- `src/platform.ts`：用 `@tauri-apps/plugin-os` 的 `platform()` 判断移动端（已缓存 + 兜底）。
- `src/App.vue`：导航在移动端改为**抽屉 + 顶栏汉堡按钮**（桌面仍是 rail + hover 展开）；加了
  `env(safe-area-inset-*)` 适配刘海/Home 指示条。
- `src/components/MobileUnsupported.vue`：桌面专属页面的替代说明页（App.vue 按路由切换）。
- `src/TasksView.vue`：任务卡加「⋮」按钮（复用原右键菜单）。
- `src/components/ConfigView.vue`：`window.prompt` 换成 Vuetify 对话框（iOS WKWebView 不支持 prompt）。
- `src/Setting.vue`：新增语言选择（English / 简体中文）；`window.prompt`/`confirm` 均已替换为 Vuetify 对话框。
- `src/BatchRenderView.vue`：`window.confirm` 换成对话框；全屏模糊在触屏设备降级。
- `src/main.ts`：移动端跳过 F12/devtools 拦截（保留右键屏蔽以抑制长按弹出菜单）。
- `src/assets/main.css`：`@media (pointer: coarse)` 降低大面积 `backdrop-filter` 模糊（移动 GPU 开销）。
- `index.html`：`viewport-fit=cover`。
- Rust：`lib.rs` 的 `asset_dir` 改为按平台/打包方式探测（Tauri 资源目录、旧版 `/usr/lib/ptkc-assets`、
  dev 的 `src-tauri/assets`）；**移动端不再因资源目录缺失而退出**——Android 的资源位于 APK 内，
  `resource_dir()` 返回 `asset://localhost/`，不是文件系统路径；iOS 的 `resource_dir()` 本身已指向
  `.../assets`。`render.rs` 增加移动端 `hw_detect` 空实现；`is_the_only_instance` 在移动端恒为 true。

## 构建注意（Apple 平台的链接）

`prpr-avc` 为 Apple 目标（`aarch64-apple-darwin`、`aarch64-apple-ios`）提供的 ffmpeg 静态库
**开了 VideoToolbox**，目标文件里引用了 `CoreVideo` / `VideoToolbox` / `CoreMedia` / `CoreFoundation`
的符号，而 `prpr-avc/build.rs` 只 `-lz`。因此仓库根目录加了 `.cargo/config.toml`，为这两个 target
补上 `-framework` 链接参数；否则 macOS dmg 与 iOS ipa 会在链接期报 undefined symbols。
未用到的 framework 会被链接器丢弃，不会影响体积。

## 已知限制 / 后续工作

- **未签名产物**：iOS 需要自签；Android release APK 未签名，装不上，需要自行签名。
- **应用图标**：移动端目前使用 Tauri 模板的默认图标。仓库里的 `src-tauri/icons/icon.png` 只有
  128×128，而 `icon.icns` 内含 1024×1024 的 PNG（`ic10` 条目）；后续可用它跑
  `pnpm tauri icon`，再把生成的 `icons/android/mipmap-*` 与 `icons/ios/AppIcon.appiconset`
  拷进 `gen/android/app/src/main/res` 与 `gen/apple/Assets.xcassets`。
- **真正的移动端渲染**：需要在应用内（in-process）用 prpr + 平台编码器重写管线，属独立项目；
  目前 prpr-avc 已带 `aarch64-apple-ios` / `aarch64-linux-android` 的 ffmpeg 静态库，是可行的起点。
- **x86_64 macOS（Universal dmg）**：上游 `TeamFlos/prpr-avc-ffmpeg` 的 target 矩阵没有
  `x86_64-apple-darwin`，全网也没有该静态库，故 macOS 只出 arm64。
