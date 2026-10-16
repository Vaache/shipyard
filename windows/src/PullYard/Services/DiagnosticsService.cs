using System.Diagnostics;
using Microsoft.UI.Dispatching;
using PullYard.Core.Util;

namespace PullYard.Services;

public static class DiagnosticsService
{
    public const double SlowUiThresholdMs = 32;
    private static readonly TimeSpan ProbeInterval = TimeSpan.FromMilliseconds(50);
    private static DispatcherQueueTimer? probe;
    private static long lastProbe;

    public static event Action? Changed;

    public static bool Enabled
    {
        get => Diagnostics.Enabled;
        set
        {
            Settings.Shared.Set(Keys.DiagnosticsEnabled, value);
            Apply(value);
            Changed?.Invoke();
        }
    }

    public static void Restore() => Apply(Settings.Shared.GetBool(Keys.DiagnosticsEnabled, false));

    private static void Apply(bool on)
    {
        Diagnostics.Enabled = on;
        if (on) StartProbe();
        else StopProbe();
    }

    private static void StartProbe()
    {
        if (probe is not null || DispatcherQueue.GetForCurrentThread() is not { } queue) return;
        var timer = queue.CreateTimer();
        timer.Interval = ProbeInterval;
        timer.IsRepeating = true;
        lastProbe = Stopwatch.GetTimestamp();
        timer.Tick += (_, _) =>
        {
            var now = Stopwatch.GetTimestamp();
            var late = Stopwatch.GetElapsedTime(lastProbe, now).TotalMilliseconds - ProbeInterval.TotalMilliseconds;
            lastProbe = now;
            if (late > SlowUiThresholdMs && late < 10000) Diagnostics.Shared.AddSlowUi(late);
        };
        timer.Start();
        probe = timer;
    }

    private static void StopProbe()
    {
        probe?.Stop();
        probe = null;
    }
}
