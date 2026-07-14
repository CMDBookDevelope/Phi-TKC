# Phi TKC
![Winamin最強! Winamin最高!](arts/phi-tklogo.png) 

Phi-TKC 是一个基于 Tauri + Vue 3 的谱面渲染工具。Fork 自: [素 ink 的 Phi-TK](https://github.com/Winamin/Phi-TK)
<!---Rust小螃蟹好可爱wwwwwwww Tauri何意味神秘图标--->

<img src="arts/logos.svg" width="120" height="60"></img>

## 下载 / Download

<div style="transform: scale(1.7);transform-origin: 0 20%;">

[![Download](https://img.shields.io/badge/Download-Latest_Release-blue?style=for-the-badge)](https://github.com/CMDBookDevelope/Phi-TKC/releases)


</div>


<small>*部分代码由AI编写/修改*</small>

## Phi-TKC 功能特性

- 支持WAV无损音频
- CRF码率控制
- 铺面揭秘功能
- 跨平台支持（Windows、Linux、macOS）
- 现代化的用户界面，基于 Vue 3 + Vuetify
- 支持Vulkan加速渲染
- 手序分配AI引擎
- 支持纯无头渲染，CLI支持完善

### 通用要求
- Rust 1.60+

## 系统要求

### 通用要求
- Node.js 18+ 
- Rust 1.60+
- pnpm

### 平台特定要求
- **Windows**: Windows 10+ 
- **Linux**: 支持现代桌面发行版
- **macOS**: macOS 10.15+

## 编译步骤

### 1. 安装依赖

#### 安装 Node.js
从 [Node.js 官网](https://nodejs.org/ "棍母") 下载并安装 Node.js 18 或更高版本。

#### 安装 Rust
从 [Rust 官网](https://rustup.rs/ "棍母") 下载并安装 Rust。

#### 安装 pnpm
```bash
npm install -g pnpm
```

### 2. 克隆仓库
```bash
git clone https://github.com/CMDBookDevelope/Phi-TKC.git
cd Phi-TKC
```

### 3. 安装项目依赖
```bash
pnpm install
```

## 编译构建

### 开发模式
```bash
pnpm tauri dev
```

### 生产构建
```bash
pnpm tauri build
```

构建完成后，可执行文件将位于：
- Windows: `src-tauri/target/release/Phi-TKC.exe`
- Linux: `src-tauri/target/release/phi-tkc`
- macOS: `src-tauri/target/release/bundle/macos/phi-tkc`

## 使用说明

1. 启动应用程序
2. 通过界面导入 Phigros 谱面文件
3. 配置渲染参数（音频质量、视频码率等）
4. 开始渲染过程
5. 导出渲染结果

## CLI

**command usage:**
<!---
```bash
phi-tk-cli # or wrapper.sh \
--input your_chart.pez [required] \
--output output.mp4 # or .mov|.mkv... [optional] \
--resolution 1920x1440 # default [optional] \
--crf 20 # default [optional] \
--fps 30 # dafault [optional] \
--dark 40 # default Background picture brightness [optional] \
--load # default=off loading screen [optional] \
--finish # default=off result screen [optional] \
--assets # assets folder [optional] \
```
--->

### 自定义参数
| 参数 | 说明 | 可用值 | 默认值 |
|------|--------|------|-----------|
| `-i, --input` | 谱面文件路径 | FilePath | 自己填。。。我怎么知道你要渲染什么，，， |
| `-o, --output` | 输出视频路径 | FilePath | output.mp4 |
| `-r, --resolution` | 分辨率 | 整数x整数 | 1920x1440 |
| `-c, --crf` | 渲染质量 | 整数 | 20 |
| `--fps` | 帧率 | 整数 | 30 |
| `-d, --dark` | 背景亮度 | 0-100 | 30 |
| `-l, --load` | 显示加载界面 | 布尔 | False |
| `--finish` | 显示结算界面 | 布尔 | False |
| `-a, --assets` | 资源文件位置 | DirectoryPath | /usr/lib/ptkc-assets |

~~*摆烂了。。。申请查看[源代码](src-tauri/src/render.rs#L1939-L1975 "1939工作室快让美芬死一死。")*~~

### 运行
```bash
./phi-tkc #后面添加渲染染变量
```

## 项目结构

``` tree
Phi-TKC/
├── src/                    # Vue 前端源码
│   ├── components/         # Vue 组件
│   ├── router/            # 路由配置
│   └── assets/            # 静态资源
├── src-tauri/             # Tauri 后端源码
│   ├── src/               # Rust 源码
│   ├── assets/            # 应用资源
│   └── icons/             # 应用图标
└── arts/                  # 文档图片
```

## 许可证

本项目采用 GNU v3.0 开源许可证，详见 LICENSE 文件。

## 相关链接
*请给这些仓库点一个免费的Star✨!*

- [Phi-TK](https://github.com/Winamin/Phi-TK "🐲")
- [Phi-TK CLI](https://github.com/CMDBookDevelope/Phi-TK-cli "🐲")
- [Phi-TK-render-lib](https://github.com/Winamin/Phi-TK-render-lib "龍龍龍")
- [Phi-TK-render-lib (CNMDBook Ver.)](https://github.com/CMDBookDevelope/Phi-TK-render-lib "🥬")
- [Macroquad CNMDBookVer.](https://github.com/CMDBookDevelope/prpr-macroquad "FULL HEADLESS SUPPORT.")
- [Miniquad CNMDBookVer.](https://github.com/CMDBookDevelope "HEADLESS nb")
- [Tauri](https://tauri.app/ "😱")
