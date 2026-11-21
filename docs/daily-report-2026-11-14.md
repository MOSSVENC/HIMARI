# 日报 2026-11-14

完成：

- 从 WSL 恢复宿主 Windows PowerShell 只读查询能力。
- 读取实时 PnP 状态，确认未知 USB 节点均为 `CM_PROB_PHANTOM`。
- 确认 `HKKbdFltr/HKMouFltr` 正在运行，并位于键盘/鼠标 `UpperFilters`。
- 确认 `FnKey.exe` 与 `neo-control.exe` 的并行输入处理风险。
- 清理历史幽灵 USB 设备实例并验证当前 Hub/xHCI/Type-C 控制器正常。
- 完成调查报告和维护者复现步骤。
- `cargo check -p himari` 通过。

未完成：

- 尚未在 Windows 重启后验证过滤器移除效果。
- 尚未进行 Windows 目标编译和实际输入回归测试。
- 尚未推送远程分支。

