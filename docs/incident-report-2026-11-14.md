# Windows 输入异常与未知 USB 设备调查报告

日期：2026-11-14  
设备：Firebat ZY-T6K  
BIOS：INSYDE Corp. 1.07.04RHQE1（2024-07-31）  
Windows：10.0.26100.9457

## 结论

本次“键盘鬼畜、Ctrl/Win 异常、窗口拖动被打断”的主要嫌疑不是 USB Hub 本身，而是键盘/鼠标类过滤驱动与用户态输入程序叠加：

- `HKKbdFltr.sys`（HotKey Keyboard Class Filter Driver）处于 `Running`，并位于键盘类 `UpperFilters` 的第一位：`HKKbdFltr, kbdclass`。
- `HKMouFltr.sys`（HotKey Mouse Class Filter Driver）处于 `Running`，并位于鼠标类 `UpperFilters` 的第一位：`HKMouFltr, mouclass`。
- 厂商 `FnKey.exe` 与改名后的 RAM CONTROL CENTER 可执行文件 `C:\ProgramData\NeoControl\neo-control.exe` 曾同时运行。
- `C:\ComboKey\profile.xml` 当前配置为 `Active="0"`、`IsKbEnable="0"`、`IsMouseEnable="0"`，没有绑定；因此没有证据表明当前配置主动映射 Ctrl/Win。
- RAM CONTROL CENTER 源码仍会在启动时无条件安装 `WH_KEYBOARD_LL` 和 `WH_MOUSE_LL` 全局钩子。即使配置为空，这仍会增加输入处理链的竞争面，是项目应修复的确定性问题。

设备管理器里的未知 USB 项目不是当前插入的 Hub：

- `USB\VID_0000&PID_0002` 的多个节点当前为 `CM_PROB_PHANTOM`；
- `USB\VID_0000&PID_0007`（硬件 ID `USB\PORT_LINK_SSINACTIVE`）当前也为 `CM_PROB_PHANTOM`；
- 当前存在的 `VID_1A40&PID_0101`、`VID_2109&PID_0211` Hub、AMD xHCI、`ACPI\ANX7411`、`ACPI\ITE8853` 均报告 `Status=OK`。

因此，“拔掉 Hub 后未知设备仍在”是因为设备管理器显示的是历史幽灵节点，不是 Hub 仍在工作的证明。

## 实时证据

在宿主 Windows PowerShell 中读取到：

```text
HKKbdFltr      Running  Manual  C:\WINDOWS\system32\DRIVERS\HKKbdFltr.sys
HKMouFltr      Running  Manual  C:\WINDOWS\system32\DRIVERS\HKMouFltr.sys

Keyboard UpperFilters = {HKKbdFltr, kbdclass}
Mouse    UpperFilters = {HKMouFltr, mouclass}

FnKey       ...\CLEVOCO.FnhotkeysandOSD_7.101.0.0...\FnKey.exe
HKClipSvc   C:\Program Files (x86)\ControlCenter\Driver\x64\HKClipSvc.exe
neo-control C:\ProgramData\NeoControl\neo-control.exe
```

未知 USB 节点查询结果：

```text
USB\VID_0000&PID_0002\...  Status=Unknown  Problem=CM_PROB_PHANTOM
USB\VID_0000&PID_0007\...  Status=Unknown  Problem=CM_PROB_PHANTOM
```

这些节点已被 `pnputil /remove-device` 清理；清理只删除了历史设备实例，没有删除当前 USB Hub、xHCI 或 Type-C 控制器。

## 根因判断

### 高可信度：厂商输入过滤器是系统级风险点

`HKKbdFltr` 和 `HKMouFltr` 是 ClassFilter，分别位于所有键盘和鼠标设备的类驱动之前。它们可以同时影响内置键盘、外置键盘、鼠标和 USB Hub 后面的 HID 设备，能解释：

- Ctrl/Win 状态错乱或重复触发；
- Windows 菜单一闪即收回；
- 鼠标拖动窗口被打断；
- 按一下内置键盘后输入状态重新同步。

### 中高可信度：软件叠加放大问题

`FnKey.exe` 使用厂商键盘/鼠标过滤能力，RAM CONTROL CENTER 又启动低级键盘/鼠标钩子。两套输入处理链同时运行时，即使 RAM CONTROL CENTER 没有绑定，也可能造成时序竞争或重复处理。当前没有证据证明 RAM CONTROL CENTER 单独完成了 Ctrl/Win 映射，因此不能把它直接定罪为唯一根因。

### 低可信度：USB Hub 是根因

当前 Hub 和控制器均为 `Status=OK`，未知 USB 设备均为幽灵节点。Hub 可能是触发条件或放大器，但现有证据不足以证明 Hub 固件损坏。

## 已完成的动作

- 查询并记录了实时 PnP、服务、过滤器注册表、进程和 USB 状态。
- 清理了四个 `CM_PROB_PHANTOM` 的历史 USB 设备实例。
- 做过一次可逆隔离测试：停止 `FnKey`、`HKClipSvc`、`neo-control` 用户态进程。
- 为输入过滤器和登录启动项创建了备份，随后已恢复到修改前状态；没有留下待重启的系统配置改动。
- Linux 目标构建已通过：`cargo check -p ram-control-center`。

## 项目需要修复的代码问题

`crates/app/src/flexikey_engine.rs` 当前在 `FlexiKeyEngine::start()` 中无条件调用 `SetWindowsHookExW(WH_KEYBOARD_LL/WH_MOUSE_LL)`。建议维护者：

1. 启动时只初始化动作表，不安装全局钩子。
2. 只有当活动 profile 中存在启用的键盘/鼠标绑定，或用户明确开始录制触发键时，才延迟安装钩子。
3. 钩子安装失败时只影响 FlexiKey，不得阻塞主 UI 或改变普通输入链。
4. 在诊断页显示“钩子未启用/已启用/安装失败”，并记录进程名、profile、绑定数量。
5. 增加 Windows 集成测试：空 profile 启动后不应出现低级钩子；启用绑定后才安装一次。
6. 文档和可执行文件的产品名统一使用当前改名后的名称，避免把旧的 `NeoControl`、`RAM CONTROL CENTER` 和厂商 `ControlCenter` 混为一谈。

本分支只保留了上述延迟安装方向的候选代码改动，尚未在 Windows 上编译和运行验证，不应直接发布为正式修复。

## 维护者复现顺序

1. 重启前先关闭 `FnKey.exe`、`HKClipSvc.exe` 和 RAM CONTROL CENTER。
2. 在设备管理器确认键盘/鼠标类仍有 `HKKbdFltr`、`HKMouFltr` UpperFilters。
3. 只启动厂商 FnKey，测试内置键盘、外接键盘和鼠标。
4. 只启动 RAM CONTROL CENTER，保持空 profile，测试输入。
5. 两者同时启动，测试输入。
6. 在可回滚备份后移除两个 ClassFilter 并重启；若异常消失，根因即得到确认。

恢复备份位置：`C:\ProgramData\NeoControl\repair-backup`。

