namespace Shipyard.Core.Api;

public sealed record RepoRef(string Owner, string Name)
{
    public string Full => $"{Owner}/{Name}";

    public static RepoRef? Parse(string? full)
    {
        if (string.IsNullOrEmpty(full)) return null;
        var i = full.IndexOf('/');
        if (i <= 0 || i == full.Length - 1) return null;
        return new RepoRef(full[..i], full[(i + 1)..]);
    }

    public override string ToString() => Full;
}

public sealed record ItemRef(RepoRef Repo, int Number)
{
    public string Title => $"{Repo.Full}#{Number}";
    public override string ToString() => Title;
}

public sealed record ProjectRef(string Id, string Title, string Url);

public enum RouteKind { PullRequest, Issue, Repo, Project, Commit }

public sealed record Route(RouteKind Kind, ItemRef? Item = null, RepoRef? Repo = null, ProjectRef? Project = null, string? Sha = null)
{
    public static Route Pr(ItemRef r) => new(RouteKind.PullRequest, Item: r, Repo: r.Repo);
    public static Route Issue(ItemRef r) => new(RouteKind.Issue, Item: r, Repo: r.Repo);
    public static Route ForRepo(RepoRef r) => new(RouteKind.Repo, Repo: r);
    public static Route ForProject(ProjectRef p) => new(RouteKind.Project, Project: p);
    public static Route Commit(RepoRef r, string sha) => new(RouteKind.Commit, Repo: r, Sha: sha);

    public string Key => Kind switch
    {
        RouteKind.PullRequest => "pr:" + Item!.Title,
        RouteKind.Issue => "issue:" + Item!.Title,
        RouteKind.Repo => "repo:" + Repo!.Full,
        RouteKind.Project => "project:" + Project!.Id,
        RouteKind.Commit => "commit:" + Repo!.Full + "@" + Sha,
        _ => ""
    };
}
