# rust-nes — 用 Rust 编写的 NES（红白机）模拟器

一个从零实现、无外部模拟器核心的 NES 模拟器：

- **CPU**：Ricoh 2A03（6502）全部官方指令 + 常见非法指令族（LAX/SAX/DCP/ISC/SLO/RLA/SRE/RRA/ARR/ANC 等），NMOS 十进制模式、NMI/IRQ/JAM 处理
- **PPU**：2C02 扫描线渲染——背景卷轴、精灵（8x8 / 8x16、优先位、溢出/0 号精灵命中标志）、调色板、灰度/增亮/颜色强调、vblank NMI、OAM DMA
- **APU**：2 脉冲 + 三角波 + 噪声通道、包络/扫频、240Hz/120Hz 帧序列器（通过 cpal 输出；DMC 未合成，属已知简化）
- **镜像加载**：iNES 与 NES 2.0 头解析（含 PRG/CHR 容量、NES2.0 尺寸编码、battery/trainer/4-screen/镜像位），无头裸 PRG 镜像自动按 NROM 处理
- **Mapper**：0 (NROM)、1 (MMC1)、2 (UxROM)、3 (CNROM)、4 (MMC3/MMC6)、7 (AxROM)、9 (MMC2)、11 (Color Dreams)、66 (MN3065)，覆盖红白机商业游戏绝大部分；不支持的 Mapper 启动时明确报错而不是黑屏
- **电池存档**：SRAM 自动写回 `<镜像名>.nes.sram`
- **启动器**：无参数启动时先选择 ROM 目录，自动列出该目录下全部游戏；选中开始、`Esc` 返回列表
- **P1/P2 键位设置**：窗口内按 **F1** 打开设置界面，可对两名玩家分别：
  - 切换输入设备：`KEYBOARD` / `GAMEPAD`
  - 选择手柄序号（多手柄时）
  - 逐键重新绑定 A/B/SELECT/START/UP/DOWN/LEFT/RIGHT（键盘按一下或手柄按一键即可捕获）
  - 音频开关
  - 修改后自动保存到 `~/rust-nes-config.json`
- **手柄**：通过 gilrs 支持 Linux/Windows/macOS 常见 XInput / DirectInput / HID 手柄

## 构建

依赖 Rust 1.75+（建议最新 stable 工具链）。所有平台通用：

```bash
cargo build --release
```

### Linux

除编译器外还需要 X11 / 音频 / 手柄相关的开发库：

```bash
# Debian/Ubuntu
sudo apt install -y libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
  libxxf86vm-dev libudev-dev libasound2-dev pkg-config
```

```bash
# Fedora
sudo dnf install -y libX11-devel libXcursor-devel libXrandr-devel libXi-devel \
  libXxf86vm-devel systemd-devel alsa-lib-devel
```

产物：`target/release/rust-nes`。全屏（`Ctrl+Enter`）通过 X11 EWMH 实现，仅在 Linux/X11 会话下生效。

### Windows

1. 用 [rustup](https://rustup.rs) 安装 Rust，默认工具链选 `stable-x86_64-pc-windows-msvc`。
2. 安装 Visual Studio Build Tools，勾选“使用 C++ 的桌面开发”（提供链接器与 Windows SDK）。
3. 不需要额外的系统库：窗口用 Win32/GDI，音频走 WASAPI，手柄走 XInput。

```powershell
cargo build --release
# 产物: target\release\rust-nes.exe
```

配置与 SRAM 路径：配置写在 `%USERPROFILE%\rust-nes-config.json`（Git Bash 等设置了 `HOME` 的环境会写到该 `HOME` 下）；SRAM 与游戏镜像放在同一目录。Windows 下暂不支持 `Ctrl+Enter` 全屏。

### macOS

1. 安装 Xcode Command Line Tools：`xcode-select --install`（提供 clang 与系统框架头文件），或安装完整 Xcode。
2. Apple Silicon 与 Intel 均可，无需安装 SDL/ALSA 之类的额外系统库：窗口走 Cocoa，音频走 CoreAudio，手柄走 IOKit/GameController。
3. 中文游戏名/目录名使用系统苹方（PingFang）字体渲染。

```bash
cargo build --release
# 产物: target/release/rust-nes
```

macOS 下暂不支持 `Ctrl+Enter` 全屏。

### 可选 feature

```bash
cargo build --release --no-default-features              # 仅键盘 + 静音（无 cpal/gilrs 依赖）
cargo build --release --features gamepad                 # 键盘 + 手柄
cargo build --release --no-default-features --features gamepad,audio
```

## 运行

```bash
./target/release/rust-nes                     # 启动器：选 ROM 目录 → 游戏列表 → 开始游戏
./target/release/rust-nes 超级马里兄弟.nes    # 直接进入指定镜像
```

不带参数启动时是一个三屏启动器：

1. **目录选择**：`↑/↓` 移动，`回车` 进入目录，移到 `[USE THIS FOLDER]` 回车确认选择；`Esc` 退出。
2. **游戏列表**：自动扫描所选目录下所有 `.nes`（不区分大小写）并按名称排序；`↑/↓` 选择，`回车` 开始游戏，`D` 重新选择 ROM 目录，`Esc` 退出程序。
3. **游戏中**：按 `Esc` 返回游戏列表（自动保存 SRAM），游戏不会退出程序。

所选目录保存在 `~/rust-nes-config.json`，下次启动直接进入游戏列表。文件名/目录名支持中文（使用系统 CJK 字体渲染，找不到系统字体时退回内置字体）。

热键：

| 键 | 功能 |
|----|------|
| F1 | 打开/关闭设置（P1/P2 键位、键盘/手柄、音频、全屏） |
| Ctrl+Enter | 切换全屏显示 |
| F2 | 复位 |
| F3 | 暂停/继续 |
| Esc | 设置界面内：关闭界面；游戏内：返回游戏列表（自动保存存档）；列表中：退出程序 |

设置界面内操作：`↑/↓` 移动，`←/→` 修改选项，`回车` 进入改键捕获（再按任意键/手柄键完成绑定，Esc 取消）。

默认键位：P1 = 方向键 + `Z/X`(B/A) + `Tab/回车`(Select/Start)；P2 = `WASD` + `G/H` + `F/T`。默认手柄布局 = 十字键 + South/East + Select/Start。

## 工程结构

```
src/
  main.rs      窗口主循环、设置界面、音频流、手柄轮询
  console.rs   总线/内存映射、OAM DMA、控制器、帧同步循环
  cpu.rs       6502 核心（含单元测试）
  ppu.rs       2C02：时序、渲染、寄存器
  apu.rs       2A03 APU：四通道 + 帧序列器 + 采样环
  mapper.rs    Mapper 0/1/2/3/4/7/9/11/66
  rom.rs       iNES / NES 2.0 解析
  config.rs    键位数据 + 序列化 + 键名/手柄键名表
  font.rs      8x8 位图字体 + 系统 CJK 字体栅格化（启动器/设置界面用）
```

## 已知边界（诚实清单）

- “所有 NES 镜像”：本模拟器覆盖最常见的 9 种 Mapper（约占商业游戏数量的 85% 以上）；MMC5、VRC 系列、BP/太阳城系等冷门 Board 未实现，加载时会提示不支持而非死机。
- APU 未合成 DMC 通道（部分游戏的采样人声/低音缺失）；音频混音为简化模型。
- PPU 为扫描线粒度渲染：一行内的中途滚动/换色（每行 IRQ 精细分割）不生效，逐行 IRQ 分割正常。
- 精灵 0 命中、溢出标志为近似时序实现。
- 未做 PALETTE 精确校准（内置近似 NTSC 调色板）。

## 运行测试 / 自检

```bash
cargo test        # 16 个单元测试：CPU 指令/BCD/中断/页跨界、PPU vblank+NMI、MMC1 串口、MMC3 8K/2K 槽位与 IRQ 等
cargo run --release -- --smoke   # 无窗口端到端自检：合成 ROM 启动→PPU 出画→vblank 循环 30 帧
```

## 许可证

本项目以 **GNU General Public License v3.0** 发布（仅该版本，不含“或更高版本”），完整协议文本见 [LICENSE](LICENSE)。

你可以自由使用、修改和分发本项目，但分发修改后的版本时须同样以 GPLv3 开放源代码。
