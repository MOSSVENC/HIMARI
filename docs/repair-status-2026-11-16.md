# 修复与验收记录（2026-11-16）

本轮先处理 Windows 输入异常、部署版本漂移和产品改名残留。功能补齐和 Flutter 界面继续列入原目标。

## 已确认的代码缺陷

1. 原来的 FlexiKey 启动流程无条件安装键盘和鼠标低级钩子；多个调用还可能重复创建线程。
2. 原来的绑定分发会吞掉绑定键的释放，即使对应按下发生在钩子启用之前。对于 Ctrl/Win 等修饰键，这会使 Windows 收不到解除状态所需的事件。
3. 录制触发键期间更新配置，会根据绑定表关闭录制仍需要的钩子；取消录制也缺少统一的恢复路径。
4. Windows 仍注册 `NeoControlBoot` 和 `NeoControl` 登录项，指向旧可执行文件。修改源码、更新 GitHub 均不会自动替换这个部署副本。
5. 卸载候选代码曾把命令行传入目录加入递归删除目标。这份草稿未作为卸载器执行；当前实现按固定文件归属清理，并拒绝路径重定向。
6. 自动注册尚未验证多客户端行为的 ACPI Fn 通知会增加与厂商常驻程序竞争的可能。监听目前由 `--trace-fn-events` 显式开启。

第 1–3 项是确定的应用缺陷；不能据此断言厂商过滤驱动与所有物理输入异常的根因均已查明。

## 实现边界

- `input_bindings.rs`：纯 Rust 的按下、重复、释放决策；只有被拦截过的按下才对应拦截释放。
- `input_hooks.rs`：钩子线程、命令确认、事件与句柄生命周期。空配置不创建钩子线程；安装失败释放本次句柄；超时线程保留到退出，避免重试叠加线程。
- `product.rs`：产品名、目录、服务、登录项与历史兼容标识。
- `service_lifecycle.rs`：安装迁移和卸载共用的限时服务移除逻辑。一次性启动服务不接受 STOP 时等待其正常退出。
- `uninstall_files.rs`：固定文件清单及路径检查，逐文件删除，空目录才移除。输入恢复备份和未知文件保留并报告。
- `scripts/deploy.ps1`：PowerShell 7 部署，校验哈希、迁移配置/驱动副本/恢复备份、注册服务与卸载入口、清理核验过的旧副本。

## 当前产品位置

```text
C:\ProgramData\RAM Control Center\ram-control-center.exe
C:\ProgramData\RAM Control Center\ram-control-center-uninstaller.exe
```

注册项：

```text
服务：RAMControlCenterBoot
HKCU\Software\Microsoft\Windows\CurrentVersion\Run → RAMControlCenter
HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\RAMControlCenter
HKLM\Software\Microsoft\Windows\CurrentVersion\App Paths\ram-control-center.exe
```

恢复备份随部署迁到 `C:\ProgramData\RAM Control Center\repair-backup`；历史启动日志迁到 `migration`。卸载器默认输出 `--plan`，`--remove` 执行清理。卸载前需从托盘退出应用；运行中的卸载器本身在重启后删除。卸载范围为当前用户和本机注册项、两个固定产品目录中的明确归属文件。其他用户离线注册表、未知快捷方式和 OEM 驱动的卸载不属于本轮自动清理范围。

## 验收标准

- 主机单元测试覆盖按键释放、重复、文件归属、未知文件和恢复备份保留，以及目录重定向拒绝。
- Windows 目标 Clippy 包含未记录 FFI 安全契约的检查。
- `--input-hooks-smoke` 先检查真实空配置不创建钩子线程，再执行 100 次键盘/鼠标句柄安装、组合、移除确认，以及安装中途失败路径；不加载用户绑定、不注入输入。
- Windows 原生运行目标平台的单元测试，另验证 GPU 后端与签名设备读取。
- 部署后核对可执行文件 SHA-256、服务退出码、登录项和旧路径残留。

## 仍需完成

- 实际冷启动后的 CPU 性能和登录体验，需要一次真正重启的证据；启动服务正常退出不等于冷启动问题已全部解决。
- 厂商 FnKey 单独运行、应用单独运行、共同运行时的物理键盘/鼠标对照验证。
- Fn 通知值到具体热键动作的映射与原生 OSD；目前的原始事件监听不是功能复刻验收。
- 触摸板快捷键等系统行为在厂商用户态程序退出后的独立性验证。DLL 导入列表只能说明直接依赖，不能证明整个行为链已独立。
- 按本机能力逐项复核灯效睡眠、防尘、设置控件的回读与交互；不能用编译通过代替这些验收。
- 完成修复和功能验收后，推进 Flutter、原创头像、视频参考和 CI Action 更新。

此前质量报告对“已验证”的描述范围过宽：编译、单元测试、部分实机读写，未覆盖常驻输入链、安装/改名/卸载生命周期和完整交互。后续每项完成声明都需关联相应的行为测试或实机证据。

## 本轮已执行结果

- 完整 `scripts/verify.sh`：格式、unsafe 边界门禁、49 项主机测试、Windows Clippy、Windows Release 全部通过。
- Windows 原生目标测试：应用 22、协议 27、卸载文件 3，共 52 项通过。
- 部署前、部署后 `--input-hooks-smoke` 均成功；`--settings-smoke` 和 `--driver-smoke` 退出码均为 0。
- 部署应用：11,085,824 字节；SHA-256 `0644ce0e2f8152b8b6d421e3acf4c2c3da1de866b5e6f75fb1139d1e1ca33d40`。
- 部署卸载器：312,832 字节；SHA-256 `4a03137f092d64efad8738f9e84b36fcb347b6296e8448f5bd1935a6027f0ab0`。
- 新启动服务一次回放耗时 8 ms，1 次尝试，服务两项退出码为 0；这仅是本次热启动服务证据。
- `NeoControlBoot` 已移除，HKCU 的 `NeoControl` 登录项已移除；新登录项指向统一目录的 `--background`。
- 旧 `C:\ProgramData\NeoControl` 目录已移除。三份恢复注册表备份与原始启动/驱动安装日志已迁入新目录，校验源/目标哈希后才删除原副本。
- 卸载器已构建并运行只读 `--plan`；本机正式卸载操作尚未执行，不能把单元测试当作完整卸载实测。

当前门禁统计为 102 个 `unsafe` 关键字，包含函数声明：钩子传输边界 5，卸载器边界 6，其余为既有 Windows/设备接口。每处安全契约接受 Clippy 检查；数字本身不是内存安全证明。

## 控件回读与温度修复

确认了三类状态覆盖缺陷：任何一次命令完成都会重置所有编辑草稿；采样期间或采样后发生的新点击可能被旧回读覆盖；首次读取失败会提前消耗全局初始化标记。现在按 25 个控制域记录请求与完成版本，采样前取版本，界面接收时再次核对。每个域取得有效数据后才完成初始化或写入确认，其余域的草稿保持原值。

临时缺失的电源、快捷键、风扇、防尘、GPU、充电读数保留已有确认值；蓝光读取使用可失败的单字节接口，避免错误路径中的零缓冲被当成实际设置。

温度原来以 CPU/GPU 任意一路有效作为整包接收条件，另一通道的零值会一起写入界面。现在各通道独立缓存并在 15 秒后过期，过期显示 `--°C`；GPU 另有 NVML 温度回退。缓存不会伪造有效采样或把失效值无限保留。

验证记录保存在本机忽略目录 `target/validation/`：

- `verify-controls-thermal.log`：57 项主机测试、格式、unsafe 门禁、Windows Clippy 和 Release 通过。
- `windows-controls-tests.log`：Windows 原生应用 30 项、协议 27 项、卸载文件 3 项，共 60 项通过。
- `windows-controls-final-smoke.log`：实际窗口属性模型回归、100 次钩子生命周期和 GPU 后端初始化检查通过。属性回归包含相反值的旧睡眠回读、首次采样恢复、草稿保留、失败写入回退、充电/GPU 临时缺失和单通道温度过期。
- `windows-controls-smoke.log`：真实 AcpiBridge 只读温度连续采样 20 次，20 次具有有效温度；覆盖约 26 秒。传感器故障与过期路径另由可控样本测试覆盖。
- `startup-task-audit.log`：按产品旧名、程序路径、FlexiKey/FlexK、CC40/FnKey/DCHU 相关命令检索计划任务，匹配 0 项。当前启动入口仍为产品服务和 HKCU Run。

这些结果覆盖状态同步和短时采样，完整功能复刻、物理输入对照和冷启动验收仍按上面的清单推进。

## 写入后的定向确认

工作线程现在在命令完成后立即读取相关控制域，避免等待下一次完整轮询才更新“应用”状态。RGB、音量等操作只触发对应的设备设置读取；风扇曲线、能力检测等查询保留在各自操作或定期完整采样中。周期采样使用独立截止时间，持续点击也会执行到期的采样。

`verify-targeted-readbacks.log` 和 `verify-bars.log` 均完成格式、57 项主机测试、unsafe 门禁、Windows Clippy 和 Release；`windows-targeted-tests.log` 中 Windows 原生 60 项测试通过。按控制域立即确认的实际交互耗时仍需单独测量，这些测试不作为点击延迟的测量结果。

## 条形控件坐标与原生像素验证

`MetricBar` 和 `ValueSlider` 的填充矩形显式固定在左上原点。RGB 滑块中心、填充末端、指针映射共用完整轨道宽度，命中区域包含两端滑块；显示比例限制在 0–100%。概览的 CPU/GPU 负载、显存、内存、磁盘，以及电池页使用同一个 `MetricBar`。

`snapshot-tools` 构建提供 `--bars-smoke=<目录>`，直接使用生产控件建立独立测试窗口，执行 10 组数值像素检查，以及三个 RGB 通道共 30 次正向、反向、越界指针移动。拖动后立即检查数值、填充起点和滑块边缘。测试只向 Slint 窗口分发指针事件，不向系统注入键鼠，也不连接硬件写入。

复现方法：在 WSL 构建 `cargo build --release --target x86_64-pc-windows-gnu -p ram-control-center --bin ram-control-center --features snapshot-tools`，然后在 Windows PowerShell 7 执行 `scripts/test-bars.ps1 -Executable <exe路径> -OutputDirectory <截图目录>`。正式交付使用默认 feature 的 Release。

本机 `windows-bars.log` 全部检查通过，渲染报告为 `GraphicsAPI::NativeOpenGL`。检查了 0%、25%、50%、100% 的截图；0% 无监视条填充，其余比例的起点始终位于轨道左端，RGB 填充终点跟随滑块中心。截图保存在本地忽略目录 `target/validation/bars/`。`windows-bars-final.log` 记录默认 Release 的状态同步、GPU 后端初始化和输入钩子生命周期检查。

本次默认 Release 的 SHA-256 为 `44cf5b937887faec22a39c9f8071b6bc6152abd85a681a8db9aa60b174c32c05`。Vulkan 后端尚未构建和测量，不据此作渲染性能对比结论。

## CI、头像与触摸板后续验证（2026-11-17）

`d9a3fde` 的 GitHub Actions 运行 `43889533402` 已完成：Linux 验证和 Windows 测试均成功。Rust 固定为 1.100.0；交叉链接器将 synchronization 导入名映射到 Zig 的 Windows API-set 库。同步 API 和图标资源编译在完整构建前独立检查。checkout 使用 v7.0.1，Zig 使用官方归档与固定 SHA-256。

原创头像以 CC0-1.0 提供 SVG、PNG 和九种尺寸 ICO。Windows 已从 Release EXE 提取出实际头像图标，应用与托盘使用同源资源。该版本已部署到统一目录，输入钩子、设置和状态同步检查通过。

触摸板控制改为读取 Windows PrecisionTouchPad 状态后执行目标状态：相同状态跳过快捷键；Windows 确认后才写厂商镜像。镜像写失败尝试恢复此前已确认的 Windows 状态。发送输入统一使用 SendInput，带 FlexiKey 标记，保留已按住的修饰键，并为部分插入准备释放序列。unsafe 总计保持 102，两个边界移入共用输入模块并相应收紧 FlexiKey 上限。

`windows-touchpad.log`：Windows 初始 Enabled=0；原值请求 0 ms；开启后 Windows 和镜像均为 true，整条链路 860 ms；重复开启 0 ms；最后恢复 Enabled=0。随后停止 FnKey.exe 做独立性对照，触摸板仍完成切换：原值请求 0 ms，关闭到开启约 6 ms，重复开启 0 ms，最后恢复 Enabled=0；测试结束后 FnKey.exe 已重新拉起。物理按住修饰键的运行时对照和完整功能覆盖仍按清单验收。

触摸板改动的本机验证：格式、64 项主机测试、Windows Clippy 与 Release 构建通过；Windows 原生测试 67 项通过（应用 37、协议 27、卸载器 3）。回归覆盖相同目标去重、延迟回读、无状态/拒绝输入、超时、镜像失败恢复、已按住修饰键以及每个部分插入前缀的释放计划。实机切换记录与单元测试分别作为行为和故障路径证据。Release SHA-256：`ed4b6e47fd46e420d923d60b1a5eb4b200b35c9d4e716e92bebf9f70182332fe`。
