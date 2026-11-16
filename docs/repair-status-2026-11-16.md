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
