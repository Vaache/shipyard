using System.Diagnostics;
using CommunityToolkit.Mvvm.ComponentModel;
using PullYard.Core;
using PullYard.Core.Update;
using PullYard.Core.Util;
using PullYard.Services;

namespace PullYard.ViewModels;

public enum UpdateState { Idle, Checking, UpToDate, Available, Downloading, Installing, ReadyToRelaunch, Failed }

public sealed partial class UpdaterViewModel : ObservableObject
{
    public static readonly TimeSpan CheckInterval = TimeSpan.FromHours(6);

    [ObservableProperty] private UpdateState state = UpdateState.Idle;
    [ObservableProperty] private Release? release;
    [ObservableProperty] private double progress;
    [ObservableProperty] private string? error;

    private bool launchCheckDone;
    private CancellationTokenSource? periodic;
    private string? stagedInstaller;

    public event Action? PresentRequested;

    public string CurrentVersion => AppInfo.Version;

    public bool IsBusy => State is UpdateState.Checking or UpdateState.Downloading or UpdateState.Installing;

    public string? AvailableVersion => State == UpdateState.Available ? Release?.Version : null;

    public string PageUrl => Release?.PageUrl ?? AppInfo.ReleasesPage;

    public bool IsInstalled => File.Exists(Path.Combine(AppContext.BaseDirectory, "unins000.exe"));

    partial void OnStateChanged(UpdateState value)
    {
        OnPropertyChanged(nameof(IsBusy));
        OnPropertyChanged(nameof(AvailableVersion));
        OnPropertyChanged(nameof(ShowsRelease));
    }

    partial void OnReleaseChanged(Release? value) => OnPropertyChanged(nameof(AvailableVersion));

    public static bool AutoCheck
    {
        get => Settings.Shared.GetBool(Keys.UpdateAutoCheck, true);
        set => Settings.Shared.Set(Keys.UpdateAutoCheck, value);
    }

    public void Present() => PresentRequested?.Invoke();

    public async Task CheckOnLaunch()
    {
        if (launchCheckDone) return;
        launchCheckDone = true;
        StartPeriodic();
        if (!AutoCheck) return;
        if (Settings.Shared.GetDate(Keys.UpdateLastCheck) is { } last && DateTimeOffset.Now - last < CheckInterval) return;
        await Check(false);
    }

    private void StartPeriodic()
    {
        if (periodic is not null) return;
        periodic = new CancellationTokenSource();
        Periodic(periodic.Token).Forget();
    }

    private async Task Periodic(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested)
        {
            try
            {
                await Task.Delay(TimeSpan.FromHours(1), ct);
            }
            catch (OperationCanceledException)
            {
                return;
            }
            if (!AutoCheck) continue;
            if (Settings.Shared.GetDate(Keys.UpdateLastCheck) is { } last && DateTimeOffset.Now - last < CheckInterval) continue;
            if (State == UpdateState.Available) continue;
            await Check(false);
        }
    }

    public async Task Check(bool userInitiated)
    {
        if (State is UpdateState.Checking or UpdateState.Downloading or UpdateState.Installing or UpdateState.ReadyToRelaunch)
        {
            if (userInitiated) Present();
            return;
        }
        State = UpdateState.Checking;
        Error = null;
        try
        {
            var latest = await Releases.FetchLatest();
            Settings.Shared.Set(Keys.UpdateLastCheck, DateTimeOffset.Now);
            Release = latest;
            if (Releases.Compare(latest.Version, CurrentVersion) > 0)
            {
                State = UpdateState.Available;
                var skipped = Settings.Shared.GetString(Keys.UpdateSkipped);
                var announced = Settings.Shared.GetString(Keys.UpdateAnnounced);
                if (userInitiated)
                {
                    Present();
                }
                else if (skipped != latest.Version && announced != latest.Version)
                {
                    Settings.Shared.Set(Keys.UpdateAnnounced, latest.Version);
                    if (AppViewModel.Shared.IsActive) Present();
                    else Notifications.Show(L.F("Update_NotificationTitle", "{0} {1} is available", AppInfo.DisplayName, latest.Version), L.T("Update_NotificationBody", "Click to see what's new and update."), "update");
                }
            }
            else
            {
                State = UpdateState.UpToDate;
                if (userInitiated) Present();
            }
        }
        catch (Exception e)
        {
            Error = e.Message;
            State = UpdateState.Failed;
            if (userInitiated) Present();
        }
    }

    public void Skip()
    {
        if (Release is null) return;
        Settings.Shared.Set(Keys.UpdateSkipped, Release.Version);
        State = UpdateState.Idle;
    }

    public void Later()
    {
        if (State is UpdateState.UpToDate or UpdateState.Failed) State = UpdateState.Idle;
    }

    public bool ShowsRelease => State switch
    {
        UpdateState.Idle or UpdateState.Checking or UpdateState.UpToDate => false,
        UpdateState.Failed => Release is { } r && Releases.Compare(r.Version, CurrentVersion) > 0,
        _ => true
    };

    public async Task DownloadAndInstall()
    {
        if (Release is not { } release || IsBusy) return;
        if (!IsInstalled || release.InstallerUrl is null)
        {
            LinkBrowser.Open(release.PageUrl);
            return;
        }
        State = UpdateState.Downloading;
        Progress = 0;
        try
        {
            var dir = Path.Combine(Path.GetTempPath(), $"{AppInfo.ExeName}-update");
            try
            {
                if (Directory.Exists(dir)) Directory.Delete(dir, true);
            }
            catch
            {
            }
            var destination = Path.Combine(dir, AppInfo.InstallerAssetName(Releases.Arch));
            var progress = new Progress<double>(p => Progress = p);
            stagedInstaller = await Releases.Download(release.InstallerUrl, destination, release.InstallerSize, progress);
            State = UpdateState.ReadyToRelaunch;
        }
        catch (Exception e)
        {
            Error = $"Couldn't download the update: {e.Message}";
            State = UpdateState.Failed;
        }
        Present();
    }

    public void Relaunch()
    {
        if (State != UpdateState.ReadyToRelaunch || stagedInstaller is null) return;
        State = UpdateState.Installing;
        try
        {
            var psi = new ProcessStartInfo(stagedInstaller)
            {
                UseShellExecute = true,
                Arguments = "/SILENT /SUPPRESSMSGBOXES /NORESTART /CLOSEAPPLICATIONS /RELAUNCH=1"
            };
            Process.Start(psi);
        }
        catch (Exception e)
        {
            Error = $"Couldn't install the update: {e.Message}";
            State = UpdateState.Failed;
            Present();
            return;
        }
        App.Quit();
    }
}
