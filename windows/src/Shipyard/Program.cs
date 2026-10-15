using System.Runtime.InteropServices;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.Windows.AppLifecycle;
using Shipyard.Core;
using Shipyard.Services;

namespace Shipyard;

public static class Program
{
    public static bool SmokeUi { get; private set; }

    [DllImport("ole32.dll")]
    private static extern uint CoWaitForMultipleObjects(uint dwFlags, uint dwMilliseconds, ulong nHandles, IntPtr[] pHandles, out uint dwIndex);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr CreateEventW(IntPtr lpEventAttributes, bool bManualReset, bool bInitialState, string? lpName);

    [DllImport("kernel32.dll")]
    private static extern bool SetEvent(IntPtr hEvent);

    [STAThread]
    private static int Main(string[] args)
    {
        if (args.Contains("--version"))
        {
            ConsoleOutput.Attach();
            Console.WriteLine($"{AppInfo.DisplayName} {AppInfo.Version}");
            return 0;
        }
        if (args.Contains("--self-test") || args.Contains("--selftest"))
        {
            ConsoleOutput.Attach();
            return SelfTest.Run().GetAwaiter().GetResult();
        }
        SmokeUi = args.Contains("--smoke-ui");
        WebViewEnvironment.Prepare();
        WinRT.ComWrappersSupport.InitializeComWrappers();
        if (!SmokeUi)
        {
            var key = AppInstance.FindOrRegisterForKey(AppInfo.AppId);
            if (!key.IsCurrent)
            {
                Redirect(AppInstance.GetCurrent().GetActivatedEventArgs(), key);
                return 0;
            }
            key.Activated += (_, e) => App.OnRedirectedActivation(e);
        }
        Application.Start(_ =>
        {
            var context = new DispatcherQueueSynchronizationContext(DispatcherQueue.GetForCurrentThread());
            SynchronizationContext.SetSynchronizationContext(context);
            _ = new App();
        });
        return App.ExitCode;
    }

    private static void Redirect(AppActivationArguments args, AppInstance key)
    {
        var handle = CreateEventW(IntPtr.Zero, true, false, null);
        Task.Run(() =>
        {
            try
            {
                key.RedirectActivationToAsync(args).AsTask().Wait();
            }
            catch
            {
            }
            SetEvent(handle);
        });
        CoWaitForMultipleObjects(0, 0xFFFFFFFF, 1, new[] { handle }, out _);
    }
}
