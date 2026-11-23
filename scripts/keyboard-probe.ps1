# Low-level keyboard probe: records what Windows receives (after the class
# filters) for each physical key, including injected/up flags and the
# GetAsyncKeyState state of the modifier keys. Read-only; it does not change
# any filter setting.
param(
    [int]$Seconds = 120,
    [string]$OutputFile = 'C:\ProgramData\Himari\input-evidence\keyboard-probe.log'
)

$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path (Split-Path $OutputFile) | Out-Null

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;

public static class KeyboardProbe {
    private const int WH_KEYBOARD_LL = 13;
    private const int WM_KEYDOWN = 0x0100;
    private static IntPtr hook = IntPtr.Zero;
    private static LowLevelKeyboardProc proc;
    private static StreamWriter log;

    [StructLayout(LayoutKind.Sequential)]
    private struct KBDLLHOOKSTRUCT {
        public uint vkCode;
        public uint scanCode;
        public uint flags;
        public uint time;
        public IntPtr dwExtraInfo;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct MSG {
        public IntPtr hwnd;
        public uint message;
        public IntPtr wParam;
        public IntPtr lParam;
        public uint time;
        public int ptX;
        public int ptY;
    }

    private delegate IntPtr LowLevelKeyboardProc(int nCode, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern IntPtr SetWindowsHookEx(int idHook, LowLevelKeyboardProc lpfn, IntPtr hMod, uint dwThreadId);
    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool UnhookWindowsHookEx(IntPtr hhk);
    [DllImport("user32.dll")]
    private static extern IntPtr CallNextHookEx(IntPtr hhk, int nCode, IntPtr wParam, IntPtr lParam);
    [DllImport("kernel32.dll")]
    private static extern IntPtr GetModuleHandle(string lpModuleName);
    [DllImport("user32.dll")]
    private static extern bool PeekMessage(out MSG lpMsg, IntPtr hWnd, uint wMsgFilterMin, uint wMsgFilterMax, uint wRemoveMsg);
    [DllImport("user32.dll")]
    private static extern bool TranslateMessage(ref MSG lpMsg);
    [DllImport("user32.dll")]
    private static extern IntPtr DispatchMessage(ref MSG lpMsg);
    [DllImport("user32.dll")]
    private static extern short GetAsyncKeyState(int vKey);

    private static string Name(uint vk) {
        switch (vk) {
            case 0x11: return "VK_CONTROL";
            case 0xA2: return "VK_LCONTROL";
            case 0xA3: return "VK_RCONTROL";
            case 0x10: return "VK_SHIFT";
            case 0xA0: return "VK_LSHIFT";
            case 0xA1: return "VK_RSHIFT";
            case 0x12: return "VK_MENU";
            case 0xA4: return "VK_LMENU";
            case 0xA5: return "VK_RMENU";
            case 0x5B: return "VK_LWIN";
            case 0x5C: return "VK_RWIN";
            default: return "VK_0x" + vk.ToString("X2");
        }
    }

    private static IntPtr Callback(int nCode, IntPtr wParam, IntPtr lParam) {
        if (nCode >= 0 && log != null) {
            KBDLLHOOKSTRUCT data = Marshal.PtrToStructure<KBDLLHOOKSTRUCT>(lParam);
            bool up = (data.flags & 0x80) != 0;
            bool injected = (data.flags & 0x10) != 0;
            short ctrl = GetAsyncKeyState(0x11);
            short lwin = GetAsyncKeyState(0x5B);
            log.WriteLine(string.Format(
                "{0:o} vk={1}({2}) scan=0x{3:X2} up={4} injected={5} async_ctrl={6} async_lwin={7} extra=0x{8:X}",
                DateTime.Now, data.vkCode, Name(data.vkCode), data.scanCode, up, injected,
                (ctrl & 0x8000) != 0, (lwin & 0x8000) != 0, data.dwExtraInfo.ToInt64()));
            log.Flush();
        }
        return CallNextHookEx(hook, nCode, wParam, lParam);
    }

    public static int Run(int seconds, string path) {
        log = new StreamWriter(path, true);
        log.WriteLine("=== keyboard probe start " + DateTime.Now.ToString("o") + " seconds=" + seconds + " ===");
        log.Flush();
        proc = Callback;
        hook = SetWindowsHookEx(WH_KEYBOARD_LL, proc, GetModuleHandle(null), 0);
        if (hook == IntPtr.Zero) {
            log.WriteLine("SetWindowsHookEx failed: " + Marshal.GetLastWin32Error());
            log.Dispose();
            return 2;
        }
        DateTime deadline = DateTime.UtcNow.AddSeconds(seconds);
        MSG msg;
        while (DateTime.UtcNow < deadline) {
            while (PeekMessage(out msg, IntPtr.Zero, 0, 0, 1)) {
                TranslateMessage(ref msg);
                DispatchMessage(ref msg);
            }
            Thread.Sleep(5);
        }
        UnhookWindowsHookEx(hook);
        log.WriteLine("=== keyboard probe stop " + DateTime.Now.ToString("o") + " ===");
        log.Dispose();
        return 0;
    }
}
'@

Write-Output "probe: $OutputFile"
exit [KeyboardProbe]::Run($Seconds, $OutputFile)
