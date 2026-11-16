# Codex 交接：Windows 输入异常与 USB 幽灵设备

请先阅读：`docs/incident-report-2026-11-14.md`。

## 已确认

- 设备为 Firebat ZY-T6K，Windows 10.0.26100.9457。
- `USB\VID_0000&PID_0002` 和 `USB\VID_0000&PID_0007` 当前都是 `CM_PROB_PHANTOM`，属于历史残留，不是当前扩展坞实体故障。
- 当前 USB Hub、AMD xHCI、ANX7411、ITE8853 均为 `Status=OK`。
- `HKKbdFltr.sys` 和 `HKMouFltr.sys` 正在运行，并分别位于键盘/鼠标类 `UpperFilters` 的第一位。
- 当时同时运行过 CLEVO `FnKey.exe`、`HKClipSvc.exe` 和改名后的 `C:\ProgramData\NeoControl\neo-control.exe`。
- `C:\ComboKey\profile.xml` 当前为禁用、无绑定状态。
- RAM CONTROL CENTER 源码在 `FlexiKeyEngine::start()` 中无条件安装 `WH_KEYBOARD_LL` 和 `WH_MOUSE_LL`，这是需要修复的设计问题。

## 根因判断

最高概率是 CLEVO 键盘/鼠标过滤驱动与 FnKey、RAM CONTROL CENTER 的多层输入处理发生状态失步。它能解释 Ctrl/Win 异常、菜单闪退、鼠标拖窗失效，以及按内置键盘后恢复。

RAM CONTROL CENTER 尚未被证明是唯一根因；它更可能是冲突参与者。USB Hub 可能是触发条件或放大器，不是目前证据支持的根因。

## 已做动作

- 清理了四个幽灵 USB 设备实例。
- 曾做过停止用户态输入程序的可逆隔离测试。
- 曾写入过滤器禁用测试配置，但已经从备份恢复；当前没有待重启的系统修改。
- 已提交调查文档，Git 提交为 `2c3b805`，尚未推送远程。

## 维护者下一步

1. Windows 目标编译并验证延迟安装 FlexiKey 钩子的改动。
2. 分别测试 FnKey 单独运行、RAM CONTROL CENTER 单独运行、两者同时运行。
3. 在可回滚备份后移除 `HKKbdFltr/HKMouFltr` 并重启，确认输入异常是否消失。
