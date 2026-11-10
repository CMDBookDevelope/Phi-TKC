# 移动端支持说明

Android 和 iOS 都已接入进程内渲染，不需要 ffmpeg 可执行文件或子进程。

| 平台 | 渲染与编码 | 发布产物 |
| --- | --- | --- |
| Android | 无窗口 Vulkan + MediaCodec H.264/HEVC、AAC + MediaMuxer MP4 | 使用 CI 临时密钥签名的 arm64 APK |
| iOS / iPadOS | 离屏 Metal + VideoToolbox H.264 + AVAssetWriter AAC/MP4 | 未签名 IPA，安装前需要自行重签 |
| 桌面 | Vulkan + 桌面编码器 / ffmpeg | Windows NSIS、Linux deb / AppImage、macOS arm64 dmg |

Metal 后端沿用渲染库的 Texture、Buffer、Pipeline、RenderPass 接口；GLSL 450 经 shaderc 和
SPIRV-Cross 转成 MSL。它创建离屏上下文，Tauri 继续管理 UIKit 窗口。支持 MSAA、深度／模板、
纹理采样与更新、裁剪、缩放和 RGBA 回读。macOS 桌面渲染仍使用 Vulkan。

## 功能

| 功能 | Android / iOS |
| --- | --- |
| 选谱、渲染、RPE、批量渲染 | 已接入本地原生任务队列 |
| 连接服务器后提交远程渲染 | 支持；默认使用本地 GPU |
| 任务列表 | 合并本地任务和已连接服务器的任务；远程任务可下载／取消 |
| 预览窗口 | 隐藏；移动端没有独立预览窗口 |
| 主题、背景、语言 | 支持 |
| 导航与任务菜单 | 底部导航和任务「⋮」按钮 |
| 自定义输出目录 | 隐藏；使用平台默认目录 |

Android 选谱使用 `ACTION_OPEN_DOCUMENT` 和 `*/*`，以支持没有 MIME 映射的谱面压缩包。
取消选择返回 `null`，不会进入解析。读取权限仍有效时，插件在后台把文件复制到独立的
`cache/charts/<uuid>` 目录；重新选择同名谱不会覆盖队列正在读取的文件。

APK 的渲染资源由插件首次渲染时复制到应用内部 `files/render-assets-<安装时间>`，再交给
渲染库按普通路径读取。Android 无头渲染不初始化 miniquad 的 Activity／窗口资源加载器。

Android 视频优先尝试保存到 Movies，系统不允许时保留应用内部输出，任务日志会给出实际路径。
iOS 输出位于 `Documents/output`，通过 Files 访问；配置已开启文件共享。

移动编码参数会规范化为 CBR：缺省 8 Mbps、偶数分辨率。iOS 当前使用 H.264、AAC 和 48 kHz
音频。任务取消和 panic 会反映在任务列表；输出旁的 `.log` 保留诊断信息。

## 构建与验证

`gen/android`、`gen/apple` 由 Tauri CLI 生成，不入库；移动端图标由 `src-tauri/icons` 提供。
Android CI 目前只发布 arm64。工作区有 Android SDK / NDK，可生成 x86_64 APK 验证模拟器；
相应的 FFmpeg 解码静态库需要单独准备，不能使用 arm64 库代替。

Apple 构建需要 Xcode。`.cargo/config.toml` 和 `src-tauri/ios-project.yml` 声明所需 framework。
桌面 workflow 的 macOS job 运行两项原生冒烟检查，再打包：

- `videotoolbox_mp4_smoke`：编码一秒 H.264 + AAC 并封装 MP4。
- miniquad `metal_offscreen`：检查 MSAA、流式缓冲快照、缩放、GL 深度／上下方向、裁剪和空采样器。

Linux 可用 `python3 scripts/check-native.py --offline` 检查本地 fork 的 Apple 代码类型，并恢复
原来的 Cargo.lock；这不能替代 Apple API 的链接和实机运行。Metal 着色器转换测试可在 Linux
用 `cargo test --features metal-shaders metal_shader` 运行。

目前工作区没有 Apple 主机或 iOS 真机；Apple 运行结果以 macOS CI／设备验证为准。


### 1.2.0 画幅

Android Vulkan／MediaCodec 和 iOS Metal／VideoToolbox 均使用设置中的完整分辨率，
不再按照谱面比例缩小宽高。旧谱面的比例字段会被忽略；宽高必须为至少 16 的偶数，
不满足时返回错误，不会自动裁掉像素。
