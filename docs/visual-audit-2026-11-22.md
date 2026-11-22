# Visual audit 2026-11-22

Every page was captured with the `snapshot-tools` build (`--snapshot=N`), which
applies the persisted theme, the capability probe and the zh-CN bundle. The
fixture runs with the DCHU device closed, so capability-driven pages show their
fallback state; that is labelled below where it matters.

| Page | Result |
|---|---|
| 0 Overview | **fixed**: 420 px accent circle at 45 % washed out FAN 3/4 and the DCHU badge; now 360 px at 18 % in the corner ([image](images/overview-2026-11-22.png)) |
| 1 Performance | two profile rows and the live result card; empty space is the no-device fixture, no overflow |
| 2 Cooling | **fixed**: Live fans values did not sit under 转速/温度/占空比 and the 占空比 label crossed the card border; `FanReadout` now shares the header widths and the 36 px padding overflow is gone ([image](images/cooling-2026-11-22.png)) |
| 3 Battery | battery card and FlexiCharger groups; empty middle is the no-battery fixture |
| 4 System | **fixed**: `Connected` and `Not located` were hard-coded English; now 已连接 / 未找到 ([image](images/system-2026-11-22.png)) |
| 5 Settings | language, theme, startup and renderer cards; no issue |
| 6 Lighting | capability fallback explains "当前平台未报告 15 色键盘能力"; no issue |
| 7 Input & I/O | blue-light disabled with a localized reason ([image](images/input-io-blue-light-disabled-2026-11-22.png)); rest enabled |
| 8 Fan curve | 20 compact cells under CPU/GPU cards; T/R labels inline, no overflow |
| 9 FlexiKey | empty profile state and the add-binding area; no issue |
| 10 Display care | taskbar toggles, monitor timeouts and screensaver chips; no issue |

Not covered by the fixture: real battery/telemetry values, lighting with a live
15-colour keyboard, and the DCHU capability list. Those need the deployed app on
the machine, not a snapshot.
