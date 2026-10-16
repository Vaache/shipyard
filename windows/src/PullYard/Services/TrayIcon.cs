using System.Runtime.InteropServices;
using PullYard.Core;

namespace PullYard.Services;

public sealed class TrayMenuItem
{
    public string Text { get; init; } = "";
    public bool Separator { get; init; }
    public bool Disabled { get; init; }
    public bool Bold { get; init; }
    public Action? Action { get; init; }
    public List<TrayMenuItem>? Children { get; init; }
}

public sealed class TrayIcon : IDisposable
{
    private delegate IntPtr WndProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct WndClassEx
    {
        public uint cbSize;
        public uint style;
        public IntPtr lpfnWndProc;
        public int cbClsExtra;
        public int cbWndExtra;
        public IntPtr hInstance;
        public IntPtr hIcon;
        public IntPtr hCursor;
        public IntPtr hbrBackground;
        public string? lpszMenuName;
        public string lpszClassName;
        public IntPtr hIconSm;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct NotifyIconData
    {
        public uint cbSize;
        public IntPtr hWnd;
        public uint uID;
        public uint uFlags;
        public uint uCallbackMessage;
        public IntPtr hIcon;

        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)]
        public string szTip;

        public uint dwState;
        public uint dwStateMask;

        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)]
        public string szInfo;

        public uint uVersion;

        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)]
        public string szInfoTitle;

        public uint dwInfoFlags;
        public Guid guidItem;
        public IntPtr hBalloonIcon;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct Point
    {
        public int X;
        public int Y;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct MenuItemInfo
    {
        public uint cbSize;
        public uint fMask;
        public uint fType;
        public uint fState;
        public uint wID;
        public IntPtr hSubMenu;
        public IntPtr hbmpChecked;
        public IntPtr hbmpUnchecked;
        public IntPtr dwItemData;
        public string? dwTypeData;
        public uint cch;
        public IntPtr hbmpItem;
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern ushort RegisterClassExW(ref WndClassEx wc);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateWindowExW(uint exStyle, string className, string windowName, uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr param);

    [DllImport("user32.dll")]
    private static extern IntPtr DefWindowProcW(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern bool DestroyWindow(IntPtr hWnd);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern uint RegisterWindowMessageW(string name);

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    private static extern bool Shell_NotifyIconW(uint message, ref NotifyIconData data);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr LoadImageW(IntPtr instance, string name, uint type, int cx, int cy, uint load);

    [DllImport("user32.dll")]
    private static extern bool DestroyIcon(IntPtr icon);

    [DllImport("user32.dll")]
    private static extern IntPtr CreatePopupMenu();

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern bool AppendMenuW(IntPtr menu, uint flags, UIntPtr id, string? text);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern bool SetMenuItemInfoW(IntPtr menu, uint item, bool byPosition, ref MenuItemInfo info);

    [DllImport("user32.dll")]
    private static extern bool SetMenuDefaultItem(IntPtr menu, uint item, uint byPos);

    [DllImport("user32.dll")]
    private static extern uint TrackPopupMenuEx(IntPtr menu, uint flags, int x, int y, IntPtr hWnd, IntPtr tpm);

    [DllImport("user32.dll")]
    private static extern bool DestroyMenu(IntPtr menu);

    [DllImport("user32.dll")]
    private static extern bool GetCursorPos(out Point point);

    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    private static extern bool PostMessageW(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr GetModuleHandleW(string? name);

    private const uint WmApp = 0x8000;
    private const uint CallbackMessage = WmApp + 1;
    private const uint WmContextMenu = 0x007B;
    private const uint NinSelect = 0x0400;
    private const uint NinKeySelect = 0x0401;
    private const uint NimAdd = 0, NimModify = 1, NimDelete = 2, NimSetVersion = 4;
    private const uint NifMessage = 1, NifIcon = 2, NifTip = 4, NifShowTip = 0x80;

    private readonly WndProc proc;
    private readonly IntPtr hwnd;
    private readonly uint taskbarCreated;
    private IntPtr icon;
    private string tooltip;
    private bool added;

    public Func<List<TrayMenuItem>>? MenuProvider { get; set; }

    public TrayIcon(string iconPath, string tooltip)
    {
        this.tooltip = tooltip;
        proc = Handle;
        var className = $"{AppInfo.ExeName}TrayWindow";
        var wc = new WndClassEx
        {
            cbSize = (uint)Marshal.SizeOf<WndClassEx>(),
            lpfnWndProc = Marshal.GetFunctionPointerForDelegate(proc),
            hInstance = GetModuleHandleW(null),
            lpszClassName = className
        };
        RegisterClassExW(ref wc);
        hwnd = CreateWindowExW(0, className, className, 0, 0, 0, 0, 0, IntPtr.Zero, IntPtr.Zero, wc.hInstance, IntPtr.Zero);
        taskbarCreated = RegisterWindowMessageW("TaskbarCreated");
        icon = LoadImageW(IntPtr.Zero, iconPath, 1, 16, 16, 0x10);
        Add();
    }

    private NotifyIconData Data(uint flags) => new()
    {
        cbSize = (uint)Marshal.SizeOf<NotifyIconData>(),
        hWnd = hwnd,
        uID = 1,
        uFlags = flags,
        uCallbackMessage = CallbackMessage,
        hIcon = icon,
        szTip = tooltip.Length > 127 ? tooltip[..127] : tooltip,
        szInfo = "",
        szInfoTitle = ""
    };

    private void Add()
    {
        var data = Data(NifMessage | NifIcon | NifTip | NifShowTip);
        added = Shell_NotifyIconW(NimAdd, ref data);
        data.uVersion = 4;
        Shell_NotifyIconW(NimSetVersion, ref data);
    }

    public void SetTooltip(string text)
    {
        tooltip = text;
        if (!added) return;
        var data = Data(NifTip | NifShowTip);
        Shell_NotifyIconW(NimModify, ref data);
    }

    private IntPtr Handle(IntPtr h, uint msg, IntPtr wParam, IntPtr lParam)
    {
        if (msg == CallbackMessage)
        {
            var evt = (uint)(lParam.ToInt64() & 0xFFFF);
            switch (evt)
            {
                case NinSelect:
                case NinKeySelect:
                case WmContextMenu:
                    ShowMenu();
                    break;
            }
            return IntPtr.Zero;
        }
        if (msg == taskbarCreated && taskbarCreated != 0)
        {
            Add();
            return IntPtr.Zero;
        }
        return DefWindowProcW(h, msg, wParam, lParam);
    }

    private void ShowMenu()
    {
        var items = MenuProvider?.Invoke() ?? new List<TrayMenuItem>();
        var actions = new Dictionary<uint, Action>();
        uint id = 100;
        IntPtr Fill(List<TrayMenuItem> list)
        {
            var popup = CreatePopupMenu();
            uint position = 0;
            foreach (var item in list)
            {
                if (item.Separator)
                {
                    AppendMenuW(popup, 0x800, UIntPtr.Zero, null);
                }
                else if (item.Children is { Count: > 0 } children)
                {
                    var sub = Fill(children);
                    AppendMenuW(popup, 0x10u | (item.Disabled ? 0x1u | 0x2u : 0u), (UIntPtr)(nuint)sub, item.Text.Replace("&", "&&"));
                }
                else
                {
                    var flags = item.Disabled ? 0x1u | 0x2u : 0u;
                    AppendMenuW(popup, flags, new UIntPtr(id), item.Text.Replace("&", "&&"));
                    if (item.Bold) SetMenuDefaultItem(popup, position, 1);
                    if (item.Action is { } a) actions[id] = a;
                    id++;
                }
                position++;
            }
            return popup;
        }
        var menu = Fill(items);
        GetCursorPos(out var pt);
        SetForegroundWindow(hwnd);
        uint chosen;
        using (DiagnosticsService.Modal()) chosen = TrackPopupMenuEx(menu, 0x0100 | 0x0080 | 0x0008, pt.X, pt.Y, hwnd, IntPtr.Zero);
        PostMessageW(hwnd, 0, IntPtr.Zero, IntPtr.Zero);
        DestroyMenu(menu);
        if (chosen != 0 && actions.TryGetValue(chosen, out var action)) action();
    }

    public void Dispose()
    {
        if (added)
        {
            var data = Data(0);
            Shell_NotifyIconW(NimDelete, ref data);
            added = false;
        }
        if (icon != IntPtr.Zero) DestroyIcon(icon);
        icon = IntPtr.Zero;
        DestroyWindow(hwnd);
    }
}
