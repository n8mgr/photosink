using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

namespace Photosink;

internal sealed class BackgroundWindow : IDisposable
{
    private const uint CallbackMessage = 0x802D;
    private static readonly uint OpenMessage = RegisterWindowMessage("com.n8mgr.photosink.open");
    private static readonly uint TaskbarCreated = RegisterWindowMessage("TaskbarCreated");
    private static readonly Guid IconId = new("ae4195b7-7f97-48ba-b201-383e7815393e");
    private readonly nint hwnd;
    private readonly SubclassProc callback;
    private readonly Action open;
    private readonly Action quit;
    private readonly nint icon;
    private readonly bool ownsIcon;
    private bool visible;
    private bool disposed;

    internal BackgroundWindow(nint hwnd, Action open, Action quit)
    {
        this.hwnd = hwnd;
        this.open = open;
        this.quit = quit;
        callback = WindowMessage;
        ExtractIconEx(Environment.ProcessPath!, 0, out var large, out var small, 1);
        if (large != 0)
        {
            DestroyIcon(large);
        }
        ownsIcon = small != 0;
        icon = ownsIcon ? small : LoadIcon(0, (nint)32512);
        if (!SetWindowSubclass(hwnd, callback, 1, 0))
        {
            if (ownsIcon)
            {
                DestroyIcon(icon);
            }
            throw new Win32Exception("Could not prepare the background window.");
        }
    }

    internal static void OpenExisting() => PostMessage((nint)0xFFFF, OpenMessage, 0, 0);

    private IconData Data() => new()
    {
        Size = (uint)Marshal.SizeOf<IconData>(),
        Window = hwnd,
        Id = 1,
        Flags = 0x01 | 0x02 | 0x04 | 0x20 | 0x80,
        CallbackMessage = CallbackMessage,
        Icon = icon,
        Tip = "Photosink",
        Guid = IconId,
        Version = 4,
    };

    internal bool SetVisible(bool value)
    {
        if (disposed || visible == value)
        {
            return visible == value;
        }
        var data = Data();
        if (value)
        {
            if (!ShellNotifyIcon(0, ref data))
            {
                return false;
            }
            ShellNotifyIcon(4, ref data);
        }
        else
        {
            ShellNotifyIcon(2, ref data);
        }
        visible = value;
        return true;
    }

    private nint WindowMessage(nint window, uint message, nuint wParam, nint lParam, nuint id, nuint data)
    {
        if (message == OpenMessage)
        {
            open();
            return 0;
        }
        if (message == TaskbarCreated && visible)
        {
            visible = false;
            SetVisible(true);
        }
        if (message == CallbackMessage)
        {
            switch ((uint)(long)lParam & 0xFFFF)
            {
                case 0x400:
                case 0x401:
                    open();
                    break;
                case 0x7B:
                    ShowMenu();
                    break;
            }
            return 0;
        }
        return DefSubclassProc(window, message, wParam, lParam);
    }

    private void ShowMenu()
    {
        var menu = CreatePopupMenu();
        if (menu == 0)
        {
            return;
        }
        try
        {
            AppendMenu(menu, 0, 1, "Open Photosink");
            AppendMenu(menu, 0, 2, "Quit");
            GetCursorPos(out var position);
            SetForegroundWindow(hwnd);
            var selected = TrackPopupMenuEx(menu, 0x0100 | 0x0080, position.X, position.Y, hwnd, 0);
            PostMessage(hwnd, 0, 0, 0);
            if (selected == 1)
            {
                open();
            }
            else if (selected == 2)
            {
                quit();
            }
        }
        finally
        {
            DestroyMenu(menu);
        }
    }

    public void Dispose()
    {
        if (disposed)
        {
            return;
        }
        SetVisible(false);
        RemoveWindowSubclass(hwnd, callback, 1);
        if (ownsIcon)
        {
            DestroyIcon(icon);
        }
        disposed = true;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct IconData
    {
        internal uint Size;
        internal nint Window;
        internal uint Id;
        internal uint Flags;
        internal uint CallbackMessage;
        internal nint Icon;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] internal string Tip;
        internal uint State;
        internal uint StateMask;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)] internal string Info;
        internal uint Version;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)] internal string InfoTitle;
        internal uint InfoFlags;
        internal Guid Guid;
        internal nint BalloonIcon;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct Point
    {
        internal int X;
        internal int Y;
    }

    [UnmanagedFunctionPointer(CallingConvention.Winapi)]
    private delegate nint SubclassProc(nint hwnd, uint message, nuint wParam, nint lParam, nuint id, nuint data);

    [DllImport("comctl32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetWindowSubclass(nint hwnd, SubclassProc callback, nuint id, nuint data);
    [DllImport("comctl32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool RemoveWindowSubclass(nint hwnd, SubclassProc callback, nuint id);
    [DllImport("comctl32.dll")]
    private static extern nint DefSubclassProc(nint hwnd, uint message, nuint wParam, nint lParam);
    [DllImport("shell32.dll", EntryPoint = "Shell_NotifyIconW", CharSet = CharSet.Unicode)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool ShellNotifyIcon(uint message, ref IconData data);
    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    private static extern uint ExtractIconEx(string file, int index, out nint large, out nint small, uint count);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern nint LoadIcon(nint instance, nint name);
    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool DestroyIcon(nint icon);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern uint RegisterWindowMessage(string message);
    [DllImport("user32.dll", EntryPoint = "PostMessageW")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool PostMessage(nint hwnd, uint message, nuint wParam, nint lParam);
    [DllImport("user32.dll")]
    private static extern nint CreatePopupMenu();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool AppendMenu(nint menu, uint flags, nuint id, string text);
    [DllImport("user32.dll")]
    private static extern uint TrackPopupMenuEx(nint menu, uint flags, int x, int y, nint hwnd, nint parameters);
    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool DestroyMenu(nint menu);
    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetCursorPos(out Point point);
    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool SetForegroundWindow(nint hwnd);
}
