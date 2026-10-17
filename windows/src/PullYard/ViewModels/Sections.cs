using PullYard.Core.Auth;
using PullYard.Core.Util;
using PullYard.Services;

namespace PullYard.ViewModels;

public enum PRFilter { ReviewRequested, Mine, Assigned, Mentioned, Reviewed }

public enum IssueFilter { Assigned, Mine, Mentioned }

public enum SectionKind { Notifications, Prs, MyPrsByRepo, Issues, Repos, Projects, Claude }

public sealed record Section(SectionKind Kind, PRFilter Pr = PRFilter.ReviewRequested, IssueFilter Issue = IssueFilter.Assigned)
{
    public static Section Notifications { get; } = new(SectionKind.Notifications);
    public static Section MyPrsByRepo { get; } = new(SectionKind.MyPrsByRepo);
    public static Section Repos { get; } = new(SectionKind.Repos);
    public static Section Projects { get; } = new(SectionKind.Projects);
    public static Section Claude { get; } = new(SectionKind.Claude);
    public static Section Prs(PRFilter f) => new(SectionKind.Prs, Pr: f);
    public static Section Issues(IssueFilter f) => new(SectionKind.Issues, Issue: f);

    public string Key => Kind switch
    {
        SectionKind.Prs => $"prs:{Pr}",
        SectionKind.Issues => $"issues:{Issue}",
        _ => Kind.ToString()
    };

    public string Title => Kind switch
    {
        SectionKind.Notifications => L.T("Section_Notifications", "Notifications"),
        SectionKind.Prs => Pr.Title(),
        SectionKind.MyPrsByRepo => L.T("Section_MyPrsByProject", "My PRs by project"),
        SectionKind.Issues => L.F("Section_IssuesTitle", "Issues: {0}", Issue.Title()),
        SectionKind.Repos => L.T("Section_Repositories", "Repositories"),
        SectionKind.Projects => L.T("Section_Projects", "Projects"),
        SectionKind.Claude => L.T("Section_Claude", "Claude Code"),
        _ => ""
    };

    public static Section? FromKey(string key)
    {
        if (key.StartsWith("prs:") && Enum.TryParse<PRFilter>(key[4..], out var p)) return Prs(p);
        if (key.StartsWith("issues:") && Enum.TryParse<IssueFilter>(key[7..], out var i)) return Issues(i);
        return Enum.TryParse<SectionKind>(key, out var k) ? new Section(k) : null;
    }
}

public static class Filters
{
    public static string Title(this PRFilter f) => f switch
    {
        PRFilter.ReviewRequested => L.T("Filter_ReviewRequests", "Review requests"),
        PRFilter.Mine => L.T("Filter_CreatedByMe", "Created by me"),
        PRFilter.Assigned => L.T("Filter_AssignedToMe", "Assigned to me"),
        PRFilter.Mentioned => L.T("Filter_Mentioned", "Mentioned"),
        _ => L.T("Filter_ReviewedByMe", "Reviewed by me")
    };

    public static string Query(this PRFilter f) => f switch
    {
        PRFilter.ReviewRequested => Demo.Query("review-requested:@me"),
        PRFilter.Mine => Demo.Query("author:@me"),
        PRFilter.Assigned => Demo.Query("assignee:@me"),
        PRFilter.Mentioned => Demo.Query("mentions:@me"),
        _ => Demo.Query("reviewed-by:@me -author:@me")
    };

    public static string Glyph(this PRFilter f) => f switch
    {
        PRFilter.ReviewRequested => "",
        PRFilter.Mine => "",
        PRFilter.Assigned => "",
        PRFilter.Mentioned => "",
        _ => ""
    };

    public static string ColorKey(this PRFilter f) => f switch
    {
        PRFilter.ReviewRequested => "sky",
        PRFilter.Mine => "violet",
        PRFilter.Assigned => "teal",
        PRFilter.Mentioned => "indigo",
        _ => "green"
    };

    public static string Title(this IssueFilter f) => f switch
    {
        IssueFilter.Assigned => L.T("Filter_AssignedToMe", "Assigned to me"),
        IssueFilter.Mine => L.T("Filter_CreatedByMe", "Created by me"),
        _ => L.T("Filter_Mentioned", "Mentioned")
    };

    public static string Query(this IssueFilter f) => f switch
    {
        IssueFilter.Assigned => Demo.Query("assignee:@me"),
        IssueFilter.Mine => Demo.Query("author:@me"),
        _ => Demo.Query("mentions:@me")
    };

    public static string Glyph(this IssueFilter f) => f switch
    {
        IssueFilter.Assigned => "",
        IssueFilter.Mine => "",
        _ => ""
    };

    public static string ColorKey(this IssueFilter f) => f switch
    {
        IssueFilter.Assigned => "teal",
        IssueFilter.Mine => "green",
        _ => "indigo"
    };
}

public enum StateFilter { Open, Closed, All }

public static class StateFilters
{
    public static string Query(this StateFilter s) => s switch
    {
        StateFilter.Open => "is:open",
        StateFilter.Closed => "is:closed",
        _ => ""
    };

    public static string Title(this StateFilter s) => s switch
    {
        StateFilter.Open => L.T("State_Open", "Open"),
        StateFilter.Closed => L.T("State_Closed", "Closed"),
        _ => L.T("State_All", "All")
    };
}

public static class ProviderText
{
    public static ForgeFilter Forge(this PRFilter f) => f switch
    {
        PRFilter.ReviewRequested => ForgeFilter.ReviewRequested,
        PRFilter.Mine => ForgeFilter.Mine,
        PRFilter.Assigned => ForgeFilter.Assigned,
        PRFilter.Mentioned => ForgeFilter.Mentioned,
        _ => ForgeFilter.Reviewed
    };

    public static PRFilter Filter(this ForgeFilter f) => f switch
    {
        ForgeFilter.ReviewRequested => PRFilter.ReviewRequested,
        ForgeFilter.Mine => PRFilter.Mine,
        ForgeFilter.Assigned => PRFilter.Assigned,
        ForgeFilter.Mentioned => PRFilter.Mentioned,
        _ => PRFilter.Reviewed
    };

    public static ForgeState Forge(this StateFilter s) => s switch
    {
        StateFilter.Open => ForgeState.Open,
        StateFilter.Closed => ForgeState.Closed,
        _ => ForgeState.All
    };

    public static bool Supports(this GitProvider p, PRFilter f) => p.SupportedFilters().Contains(f.Forge());

    public static string RequestsTitle(this GitProvider p) => p.UsesMergeRequests() ? L.T("Provider_MergeRequests", "Merge requests") : L.T("Nav_PullRequests", "Pull requests");

    public static string RequestTitle(this GitProvider p) => p.UsesMergeRequests() ? L.T("Provider_MergeRequest", "merge request") : L.T("Provider_PullRequest", "pull request");

    public static string RequestShort(this GitProvider p) => p.UsesMergeRequests() ? L.T("Provider_MR", "MR") : L.T("Provider_PR", "PR");

    public static string TokenTitle(this GitProvider p) => p == GitProvider.Bitbucket ? L.T("Login_ApiTokenTitle", "API token") : L.T("Login_PatTitle", "Personal Access Token");

    public static string TokenHint(this GitProvider p) => p switch
    {
        GitProvider.GitHub or GitProvider.GitHubEnterprise => L.T("Login_PatBody", "Required scopes: repo, read:org, read:project, project, notifications, workflow."),
        GitProvider.GitLab => L.T("Login_GitLabTokenBody", "Required scope: api (or read_api for read-only)."),
        GitProvider.Bitbucket => L.T("Login_BitbucketTokenBody", "Use your Atlassian account email and an API token with Bitbucket read scopes for account, repositories and pull requests."),
        _ => L.T("Login_GiteaTokenBody", "Required permissions: read for user, repository and issue.")
    };

    public static string DeviceHelp(this GitProvider p) => p == GitProvider.GitLab
        ? L.T("Login_GitLabDeviceBody", "Requires GitLab 17.1 or later and the Application ID of your own OAuth application (User settings → Applications): not confidential, scope api, “Device authorization grant” enabled.")
        : L.T("Login_DeviceBody", "Requires the Client ID of your own OAuth App (Settings → Developer settings → OAuth Apps, enable “Enable Device Flow”).");

    public static string Glyph(this GitProvider p) => p switch
    {
        GitProvider.GitHub => "",
        GitProvider.GitHubEnterprise => "",
        GitProvider.GitLab => "",
        GitProvider.Bitbucket => "",
        _ => ""
    };

    public static string ColorKey(this GitProvider p) => p switch
    {
        GitProvider.GitHub => "violet",
        GitProvider.GitHubEnterprise => "indigo",
        GitProvider.GitLab => "coral",
        GitProvider.Bitbucket => "sky",
        _ => "green"
    };
}
