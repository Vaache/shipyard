using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;
using Windows.ApplicationModel.DataTransfer;
using Windows.Storage.Pickers;

namespace PullYard.Services;

public static class WindowInterop
{
    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    private static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);

    [DllImport("user32.dll")]
    private static extern bool IsIconic(IntPtr hWnd);

    [StructLayout(LayoutKind.Sequential)]
    private struct FlashInfo
    {
        public uint cbSize;
        public IntPtr hwnd;
        public uint dwFlags;
        public uint uCount;
        public uint dwTimeout;
    }

    [DllImport("user32.dll")]
    private static extern bool FlashWindowEx(ref FlashInfo info);

    public static IntPtr Handle(Window window) => WinRT.Interop.WindowNative.GetWindowHandle(window);

    public static void BringToFront(Window window)
    {
        var hwnd = Handle(window);
        if (IsIconic(hwnd)) ShowWindow(hwnd, 9);
        else ShowWindow(hwnd, 5);
        window.Activate();
        SetForegroundWindow(hwnd);
    }

    public static void Flash(Window window)
    {
        var info = new FlashInfo { cbSize = (uint)Marshal.SizeOf<FlashInfo>(), hwnd = Handle(window), dwFlags = 3 | 12, uCount = 3, dwTimeout = 0 };
        FlashWindowEx(ref info);
    }

    public static async Task<string?> PickFolder(Window window)
    {
        var picker = new FolderPicker { SuggestedStartLocation = PickerLocationId.DocumentsLibrary };
        picker.FileTypeFilter.Add("*");
        WinRT.Interop.InitializeWithWindow.Initialize(picker, Handle(window));
        using var modal = DiagnosticsService.Modal();
        var folder = await picker.PickSingleFolderAsync();
        return folder?.Path;
    }

    public static async Task<List<string>> PickImages(Window window)
    {
        var picker = new FileOpenPicker { SuggestedStartLocation = PickerLocationId.PicturesLibrary };
        foreach (var ext in new[] { ".png", ".jpg", ".jpeg", ".gif", ".webp" }) picker.FileTypeFilter.Add(ext);
        WinRT.Interop.InitializeWithWindow.Initialize(picker, Handle(window));
        using var modal = DiagnosticsService.Modal();
        var files = await picker.PickMultipleFilesAsync();
        return files?.Select(f => f.Path).ToList() ?? new List<string>();
    }

    public static async Task<string?> PickSaveFile(Window window, string suggestedName, string typeName, string extension)
    {
        var picker = new FileSavePicker { SuggestedStartLocation = PickerLocationId.DocumentsLibrary, SuggestedFileName = suggestedName };
        picker.FileTypeChoices.Add(typeName, new List<string> { extension });
        WinRT.Interop.InitializeWithWindow.Initialize(picker, Handle(window));
        using var modal = DiagnosticsService.Modal();
        var file = await picker.PickSaveFileAsync();
        return file?.Path;
    }

    public static void Copy(string text)
    {
        try
        {
            var package = new DataPackage();
            package.SetText(text);
            Clipboard.SetContent(package);
            Clipboard.Flush();
        }
        catch
        {
        }
    }

    public static async Task<string?> ReadClipboardText()
    {
        try
        {
            var content = Clipboard.GetContent();
            if (content.Contains(StandardDataFormats.Text)) return await content.GetTextAsync();
        }
        catch
        {
        }
        return null;
    }
}
