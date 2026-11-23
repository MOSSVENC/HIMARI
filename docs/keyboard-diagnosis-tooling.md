# 键盘/过滤器取证工具与验证顺序

面向“开机后外置键盘 Ctrl/Win 错乱、内置键盘一按恢复”的专项工具。所有工具默认只读；只有显式传入动作才会写过滤器。

## 仓库内工具

### `scripts/keyboard-evidence.ps1`

在**开机后、按内置键盘之前**运行，抓一份带时间戳的快照：

- 启动时间、`HKKbdFltr`/`HKMouFltr`/`HimariBoot`/DCHU 服务状态；
- 键盘、鼠标类 `UpperFilters`；
- `FnKey`、`HKClipSvc`、`CC40`、HIMARI 进程及启动时间；
- 键盘设备、父设备、端口位置、驱动服务；
- USB Hub 与当前故障设备；
- StickyKeys/FilterKeys、Scancode Map、布局、Fast Startup；
- `input-guard.log` 尾部并复制整份日志；
- 最近两小时的 Kernel-Boot/Kernel-Power/Service Control/HID/USB 事件。

输出到 `C:\ProgramData\Himari\input-evidence\`，文本和 JSON 各一份。

### `scripts/keyboard-probe.ps1`

低级键盘钩子探针，记录 Windows 在类过滤器之后实际收到的内容：

```text
时间 vk=0x11(VK_CONTROL) scan=0x1D up=False injected=False async_ctrl=True async_lwin=False
```

用法：

```powershell
pwsh -ExecutionPolicy Bypass -File scripts\keyboard-probe.ps1 -Seconds 120
```

建议在开机后按顺序做：

1. 先按外接键盘的 Ctrl、Win 各三次；
2. 再按一下内置键盘；
3. 再按外接 Ctrl、Win 各三次。

判定：如果外接 Ctrl 在**按内置键盘之前**就被记录为 `VK_LWIN`，说明翻译发生在 HID/类过滤器/固件层，不在应用层；如果记录为 `VK_CONTROL` 但应用表现像 Win，则转向布局/辅助功能/第三方映射软件。

局限：低级钩子看到的是类过滤器之后的结果，不能证明是哪一个过滤器改的，也不能区分设备。

### `himari.exe --keyboard-filter-probe <action>`

直接向签名键盘过滤器发控制码，用于受控实验：

| 动作 | 含义 |
|---|---|
| `safe` | USB 键盘移出过滤链，再关闭拦截（守护的默认动作） |
| `disable` / `enable` | 关闭/开启过滤器拦截 |
| `usb-off` / `usb-on` | 让 USB 键盘移出/进入过滤链 |
| `clear` | 清空过滤器宏表 |
| `boot-state` | 模拟开机早期：USB 进入过滤链 + 开启拦截 |

每次写入都是盲写，设备没有读回；`boot-state` 可能复现异常，做完后应立即执行 `safe`。

## 外部工具

- **Sysinternals**：Process Monitor（看 `HKClipSvc`/`FnKey` 对过滤器设备的操作）、Autoruns（核对 UpperFilters 与启动项）、DebugView（若驱动有 DbgPrint）。
- **USB Device Tree Viewer**：核对键盘挂在哪一级 Hub、端口、电源状态。
- **USBPcap + Wireshark**：抓 USB HID 报告，比较外接键盘在异常前后发出的 modifier 位；需要装抓包驱动，属于较重手段。
- **wpr / Windows Performance Analyzer**：ETW 抓 `Microsoft-Windows-HID*`、USB Hub、Win32k 输入事件，适合分析开机时序。
- **PowerToys Keyboard Manager**：检查/排除用户态键位映射；它自己也会改键，测试时要先禁用。
- **AutoHotkey**：用 Key History 看系统收到的按键历史；与低级钩子类似，属于用户态。
- **在线键盘测试**（keyboardtest.cn、keyboard-tester.com）：验证浏览器能收到哪些键；输入已经错乱时不一定能打开浏览器，只作辅助。
- **微软 Q&A / Superuser / Reddit**：检索 `HKKbdFltr`、HotKey Keyboard Class Filter Driver 的同类案例；目前没有找到同机型定论，只能作旁证。

## 最小验证顺序

1. 下一次重启，先跑 `keyboard-evidence.ps1`，不要碰内置键盘。
2. 跑 `keyboard-probe.ps1`，按外接 Ctrl/Win → 内置键盘一次 → 外接 Ctrl/Win。
3. 对照 `input-guard.log`：修复行是否在第一次按键之前出现。
4. 若修复行已在之前但问题仍出现：先 `--keyboard-filter-probe clear`，再测；仍复现则用 `boot-state` 做受控复现，随后 `safe`。
5. 若确认过滤器：备份键盘类 UpperFilters，只移除 `HKKbdFltr` 重启对照；恢复后再单独查 `HKMouFltr`。
6. 最后做设备隔离（拔无线接收器、绕过 Hub）和 Fast Startup 对照。

## 尚未覆盖

- `HKMouFltr` 没有对应的 USB 旁路控制；鼠标拖动异常可能仍来自它。
- 低级钩子不能归因到具体设备；Raw Input + `GetRawInputDeviceInfo` 的探针尚未实现。
- `input-guard.log` 没有时间戳，开机时刻只能靠文件时间和邻近日志推断。
