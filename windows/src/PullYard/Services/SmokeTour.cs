using Microsoft.UI.Windowing;
using PullYard.Core;
using PullYard.Core.Api;
using PullYard.ViewModels;
using PullYard.Views;

namespace PullYard.Services;

public static class SmokeTour
{
    private static string LogPath => Path.Combine(AppInfo.DataDirectory, "tour.log");

    private static void Log(string text)
    {
        try
        {
            File.AppendAllText(LogPath, $"{DateTimeOffset.Now:HH:mm:ss.fff} {text}\n");
        }
        catch
        {
        }
    }

    public static async Task Run(MainWindow window)
    {
        var app = AppViewModel.Shared;
        try
        {
            if (window.AppWindow.Presenter is OverlappedPresenter presenter) presenter.Maximize();
            Log("start");
            await Step("launch", 9000);
            if (app.Phase != Phase.LoggedIn)
            {
                Log($"phase {app.Phase}");
                await Step("login view", 3000);
                Finish(0);
                return;
            }
            var pr = app.DetailRoot;
            Log($"detail {pr?.Key ?? "none"}");
            await Step("pr conversation", 9000);
            if (pr is { Kind: RouteKind.PullRequest, Item: { } item } && app.PrModel(item) is { } model)
            {
                model.Tab = PRTab.Files;
                await Step("pr files", 12000);
                model.Tab = PRTab.Commits;
                await Step("pr commits", 3000);
                model.Tab = PRTab.Checks;
                await Step("pr checks", 4000);
                model.Tab = PRTab.Conversation;
            }
            app.Section = Section.MyPrsByRepo;
            await Step("prs by project", 5000);
            app.Section = Section.Issues(IssueFilter.Assigned);
            await Step("issues", 4000);
            app.Section = Section.Notifications;
            await Step("notifications", 3000);
            if (pr?.Repo is { } repo)
            {
                app.Section = Section.Repos;
                app.Select(Route.ForRepo(repo));
                await Step("repo code", 8000);
            }
            app.Section = Section.Projects;
            await Step("projects", 3000);
            app.Section = Section.Claude;
            await Step("claude", 5000);
            app.Section = Section.Prs(PRFilter.ReviewRequested);
            app.RequestSettings();
            await Step("settings", 4000);
            Finish(App.ExitCode);
        }
        catch (Exception e)
        {
            Log("error " + e);
            Finish(5);
        }
    }

    private static async Task Step(string name, int ms)
    {
        Log("stage " + name);
        await Task.Delay(ms);
    }

    private static void Finish(int code)
    {
        Log($"finish {code}");
        App.ExitCode = code;
        App.Quit();
    }
}
