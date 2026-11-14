# 提交日志 2026-11-14

建议提交信息：

```text
fix(flexikey): defer global input hooks until profile use
```

提交范围：

- `crates/app/src/flexikey_engine.rs`：空 profile 启动时不再无条件安装低级键盘/鼠标钩子；有绑定或开始录制时延迟安装。
- `docs/incident-report-2026-11-14.md`：记录 Windows 实时证据、根因判断、回滚边界和维护者复现步骤。
- `docs/daily-report-2026-11-14.md`：记录本日工作和未完成验证。

验证：

```text
cargo check -p ram-control-center
```

结果：通过（Linux 目标）。Windows 目标和重启后的输入回归仍需维护者完成。

本次没有提交未相关的工作树修改，也没有推送远程仓库。

