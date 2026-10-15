using Shipyard.Core.Util;
using Shipyard.Services;

namespace Shipyard.ViewModels;

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
