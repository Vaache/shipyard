using System.Diagnostics;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Windowing;
using PullYard.Core.Util;

namespace PullYard.Services;

public static class DiagnosticsService
{
    public const double SlowUiThresholdMs = 16;
    private static readonly TimeSpan ProbeInterval = TimeSpan.FromMilliseconds(50);
    private static DispatcherQueueTimer? probe;
    private static long lastProbe;
    private static bool skipNext;
    private static int modalDepth;

    public static event Action? Changed;

    public static bool Enabled
    {
        get => Diagnostics.Enabled;
        set
        {
            Settings.Shared.Set(Keys.DiagnosticsEnabled, value);
            SetRecording(value);
        }
    }

    public static void Restore() => SetRecording(Settings.Shared.GetBool(Keys.DiagnosticsEnabled, false));

    public static void SetRecording(bool on)
    {
        Diagnostics.Enabled = on;
        if (on) StartProbe();
        else StopProbe();
        Changed?.Invoke();
    }

    private static bool MainWindowShown()
    {
        if (App.MainWindow?.AppWindow is not { } window) return true;
        if (!window.IsVisible) return false;
        return window.Presenter is not OverlappedPresenter { State: OverlappedPresenterState.Minimized };
    }

    private static void StartProbe()
    {
        if (probe is not null || DispatcherQueue.GetForCurrentThread() is not { } queue) return;
        var timer = queue.CreateTimer();
        timer.Interval = ProbeInterval;
        timer.IsRepeating = true;
        lastProbe = Stopwatch.GetTimestamp();
        skipNext = true;
        timer.Tick += (_, _) => OnProbe();
        timer.Start();
        probe = timer;
    }

    private static void OnProbe()
    {
        var now = Stopwatch.GetTimestamp();
        var late = Stopwatch.GetElapsedTime(lastProbe, now).TotalMilliseconds - ProbeInterval.TotalMilliseconds;
        lastProbe = now;
        if (modalDepth > 0 || !MainWindowShown())
        {
            skipNext = true;
            return;
        }
        if (skipNext)
        {
            skipNext = false;
            return;
        }
        if (late > SlowUiThresholdMs) Diagnostics.Shared.AddSlowUi(late);
    }

    public static IDisposable Modal()
    {
        modalDepth++;
        return new ModalScope();
    }

    private sealed class ModalScope : IDisposable
    {
        private bool done;

        public void Dispose()
        {
            if (done) return;
            done = true;
            modalDepth--;
            skipNext = true;
        }
    }

    private static void StopProbe()
    {
        probe?.Stop();
        probe = null;
    }
}
