using System.ComponentModel;
using System.Text.Json.Nodes;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Shipyard.Controls;
using Shipyard.Core.Api;
using Shipyard.Core.Claude;
using Shipyard.Core.Html;
using Shipyard.Core.Util;
using Shipyard.Services;
using Shipyard.ViewModels;
using Windows.System;
using VisualState = Shipyard.Core.Api.VisualState;

namespace Shipyard.Views;

public sealed class PRDetailView : Grid
{
    private readonly AppViewModel app = AppViewModel.Shared;
    private readonly ItemRef itemRef;
    private PRDetailViewModel? model;
    private readonly ContentControl root = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch, IsTabStop = false };
    private PRContent? content;
    private bool subscribed;

    public PRDetailView(ItemRef r)
    {
        itemRef = r;
        Children.Add(root);
        root.Content = Ui.Loading();
        Loaded += (_, _) =>
        {
            if (subscribed) return;
            subscribed = true;
            app.RefreshRequested += OnRefresh;
            app.LiveTick += OnLive;
            Start();
        };
        Unloaded += (_, _) =>
        {
            subscribed = false;
            app.RefreshRequested -= OnRefresh;
            app.LiveTick -= OnLive;
            if (model is not null) model.PropertyChanged -= OnModel;
        };
    }

    private void OnRefresh() => _ = model?.Load(true);

    private void OnLive() => _ = model?.Load();

    private void Start()
    {
        model ??= app.PrModel(itemRef);
        if (model is null) return;
        model.PropertyChanged -= OnModel;
        model.PropertyChanged += OnModel;
        Update();
        _ = model.Load();
    }

    private void OnModel(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(PRDetailViewModel.Pr) or nameof(PRDetailViewModel.Error) or nameof(PRDetailViewModel.NotPR)) Update();
    }

    private void Update()
    {
        if (model is null) return;
        if (model.NotPR)
        {
            root.Content = new IssueDetailView(itemRef);
            return;
        }
        if (model.Pr is not null)
        {
            if (content is null)
            {
                content = new PRContent(model);
                root.Content = content;
            }
            return;
        }
        root.Content = model.Error is { } e ? Ui.Error(e, () => _ = model.Load()) : Ui.Loading();
    }
}

public sealed class PRContent : Grid
{
    private readonly AppViewModel app = AppViewModel.Shared;
    private readonly PRDetailViewModel model;
    private readonly StackPanel header = Ui.V(8);
    private readonly StackPanel banners = Ui.V(6);
    private readonly TabStrip tabs = new(nameof(PRTab.Conversation));
    private readonly Grid tabArea = new();
    private readonly StackPanel toolbar = Ui.H(2);
    private readonly RefreshFooter status = new(compact: true);
    private readonly ProgressRing busy = Ui.Spinner(16);
    private readonly Dictionary<PRTab, FrameworkElement> tabViews = new();
    private bool stackExpanded;
    private HtmlView? conversation;
    private HtmlView? diff;
    private ListView? fileList;
    private TextBox? fileFilter;
    private TextBlock? viewedCount;
    private int lastScrollToken;
    private int renderedDiffVersion = -1;
    private string renderedFilter = "";
    private string lastHtml = "";

    public PRContent(PRDetailViewModel model)
    {
        this.model = model;
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });

        var top = Ui.Columns((header, Ui.Star), (Ui.H(4, busy, status, toolbar), Ui.Auto));
        top.Padding = new Thickness(16, 12, 12, 0);
        ((FrameworkElement)top.Children[1]).VerticalAlignment = VerticalAlignment.Top;
        Children.Add(top);
        banners.Padding = new Thickness(16, 6, 16, 0);
        SetRow(banners, 1);
        Children.Add(banners);
        var tabRow = Ui.V(0, tabs, Ui.Line());
        tabRow.Margin = new Thickness(0, 6, 0, 0);
        SetRow(tabRow, 2);
        Children.Add(tabRow);
        SetRow(tabArea, 3);
        Children.Add(tabArea);

        tabs.SelectionChanged += id => model.Tab = Enum.Parse<PRTab>(id);
        status.Retry = () => _ = model.Load(true);
        busy.Visibility = Visibility.Collapsed;
        model.PropertyChanged += OnModel;
        Unloaded += (_, _) => model.PropertyChanged -= OnModel;
        Loaded += (_, _) =>
        {
            model.PropertyChanged -= OnModel;
            model.PropertyChanged += OnModel;
            RenderAll();
        };
        AddShortcuts();
        RenderAll();
    }

    private PullRequestDetail Pr => model.Pr!;

    private void AddShortcuts()
    {
        var index = 0;
        foreach (var tab in Enum.GetValues<PRTab>())
        {
            var t = tab;
            var acc = new KeyboardAccelerator { Key = VirtualKey.Number1 + index, Modifiers = VirtualKeyModifiers.Control | VirtualKeyModifiers.Shift };
            acc.Invoked += (_, e) =>
            {
                e.Handled = true;
                model.Tab = t;
            };
            KeyboardAccelerators.Add(acc);
            index++;
        }
    }

    private void OnModel(object? sender, PropertyChangedEventArgs e)
    {
        switch (e.PropertyName)
        {
            case nameof(PRDetailViewModel.Pr):
                RenderAll();
                break;
            case nameof(PRDetailViewModel.Stack):
            case nameof(PRDetailViewModel.Repo):
                RenderHeader();
                RenderBanners();
                break;
            case nameof(PRDetailViewModel.Tab):
                ShowTab();
                break;
            case nameof(PRDetailViewModel.Html):
                RenderConversation();
                break;
            case nameof(PRDetailViewModel.ScrollToBottomToken):
                if (model.ScrollToBottomToken != lastScrollToken)
                {
                    lastScrollToken = model.ScrollToBottomToken;
                    conversation?.ScrollToBottom();
                }
                break;
            case nameof(PRDetailViewModel.Loading):
            case nameof(PRDetailViewModel.LastUpdated):
            case nameof(PRDetailViewModel.RefreshError):
                status.Update(model.LastUpdated, model.Loading, model.RefreshError);
                break;
            case nameof(PRDetailViewModel.Busy):
                busy.Visibility = model.Busy ? Visibility.Visible : Visibility.Collapsed;
                break;
            case nameof(PRDetailViewModel.ActionError):
                if (model.ActionError is { } err)
                {
                    model.ActionError = null;
                    _ = Dialogs.Alert(XamlRoot, L.T("Common_Error", "Error"), err);
                }
                break;
            case nameof(PRDetailViewModel.DiffVersion):
            case nameof(PRDetailViewModel.Files):
            case nameof(PRDetailViewModel.FilesLoaded):
            case nameof(PRDetailViewModel.FilesError):
                RenderFiles();
                break;
            case nameof(PRDetailViewModel.ScrollTarget):
                if (model.ScrollTarget is { } path)
                {
                    diff?.Send(new JsonObject { ["op"] = "scrollToFile", ["path"] = path });
                    if (fileList is not null) fileList.SelectedItem = (fileList.ItemsSource as IEnumerable<FileListItem>)?.FirstOrDefault(f => f.File.Id == path);
                }
                break;
            case nameof(PRDetailViewModel.PendingCount):
                RenderToolbar();
                break;
        }
    }

    private void RenderAll()
    {
        if (model.Pr is null) return;
        RenderHeader();
        RenderBanners();
        RenderToolbar();
        RenderTabs();
        ShowTab();
        RenderConversation();
        if (tabViews.ContainsKey(PRTab.Commits)) tabViews[PRTab.Commits] = BuildCommits(tabViews[PRTab.Commits]);
        if (tabViews.ContainsKey(PRTab.Checks)) tabViews[PRTab.Checks] = BuildChecks(tabViews[PRTab.Checks]);
        status.Update(model.LastUpdated, model.Loading, model.RefreshError);
    }

    private void RenderHeader()
    {
        if (model.Pr is null) return;
        var pr = Pr;
        header.Children.Clear();
        var title = Ui.H(8, Ui.Text(pr.Title, 20, "text", FontWeights.Bold, wrap: true, select: true), Ui.Text($"#{pr.Number}", 20, "dim"));
        ((TextBlock)title.Children[0]).MaxWidth = 900;
        header.Children.Add(title);
        var branchBase = BranchChip(pr.BaseRefName, "sky");
        var branchHead = BranchChip(pr.HeadRefName, "violet");
        var time = Ui.Caption(Fmt.Rel(pr.CreatedAt));
        Ui.Tip(time, Fmt.Full(pr.CreatedAt));
        header.Children.Add(Ui.H(8, Ui.StateBadge(pr.VisualState), Ui.Avatar(pr.Author?.AvatarUrl, 18), Ui.Text(pr.Author?.Login ?? "ghost", 13, "text", FontWeights.SemiBold),
            branchBase, Ui.Glyph("", 11, "dim"), branchHead, Ui.DiffStat(pr.Additions, pr.Deletions), time));
        var reviewers = model.Reviewers;
        if (pr.Labels.Items.Count > 0 || reviewers.Count > 0 || pr.Assignees.Items.Count > 0)
        {
            var row = Ui.H(6);
            foreach (var l in pr.Labels.Items) row.Children.Add(Ui.LabelChip(l));
            if (reviewers.Count > 0)
            {
                row.Children.Add(Ui.Caption(L.T("PR_Reviewers", "Reviewers:")));
                foreach (var r in reviewers) row.Children.Add(ReviewerBadge(r));
            }
            if (pr.Assignees.Items.Count > 0)
            {
                row.Children.Add(Ui.Caption(L.T("PR_Assignees", "Assignees:")));
                foreach (var a in pr.Assignees.Items)
                {
                    var av = Ui.Avatar(a.AvatarUrl, 18);
                    Ui.Tip(av, a.Login);
                    row.Children.Add(av);
                }
            }
            header.Children.Add(new ScrollViewer { Content = row, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalScrollBarVisibility = ScrollBarVisibility.Disabled, HorizontalScrollMode = ScrollMode.Enabled, VerticalScrollMode = ScrollMode.Disabled });
        }
        if (model.Stack.Count > 1) header.Children.Add(StackStrip());
        if (MergeStatusBar(model, app, () => _ = Dialogs.Merge(XamlRoot, model)) is { } bar) header.Children.Add(bar);
    }

    public static Border BranchChip(string name, string color)
    {
        var chip = Ui.Chip(name, color, null, mono: true);
        chip.Tapped += (_, _) => WindowInterop.Copy(name);
        Ui.Tip(chip, L.T("Common_ClickToCopy", "Click to copy"));
        return chip;
    }

    public static FrameworkElement ReviewerBadge(ReviewerInfo r)
    {
        var (glyph, color, title) = r.State switch
        {
            "APPROVED" => ("", "green", "approved"),
            "CHANGES_REQUESTED" => ("", "red", "changes requested"),
            "COMMENTED" => ("", "muted", "commented"),
            "DISMISSED" => ("", "muted", "dismissed"),
            _ => ("", "amber", "review pending")
        };
        var p = Ui.H(3, Ui.Avatar(r.Avatar, 18), Ui.Glyph(glyph, r.State is "APPROVED" or "CHANGES_REQUESTED" or "COMMENTED" ? 11 : 6, color));
        Ui.Tip(p, $"{r.Login}: {title}");
        return p;
    }

    private FrameworkElement StackStrip()
    {
        var stack = model.Stack;
        var current = Pr.Number;
        var index = Math.Max(0, stack.FindIndex(s => s.Number == current));
        var dots = Ui.H(3);
        foreach (var s in stack)
        {
            dots.Children.Add(new Border
            {
                Width = s.Number == current ? 18 : 10,
                Height = 5,
                CornerRadius = new CornerRadius(2.5),
                Background = s.Number == current ? Palette.Brush("indigo") : Palette.Brush(Palette.StateKey(s.VisualState), 0.55),
                VerticalAlignment = VerticalAlignment.Center
            });
        }
        var head = Ui.H(8, Ui.Glyph("", 13, "indigo"), Ui.Text(L.T("PR_Stack", "Stack"), 13, "text", FontWeights.SemiBold), Ui.Caption(L.F("PR_StackPosition", "{0} of {1}", index + 1, stack.Count)), dots);
        var nav = Ui.H(4);
        if (index > 0) nav.Children.Add(StackNav(stack[index - 1], "", L.T("PR_DownStack", "Down the stack")));
        if (index < stack.Count - 1) nav.Children.Add(StackNav(stack[index + 1], "", L.T("PR_UpStack", "Up the stack")));
        nav.Children.Add(Ui.IconButton(stackExpanded ? "" : "", L.T("PR_ShowStack", "Show stack"), () =>
        {
            stackExpanded = !stackExpanded;
            RenderHeader();
        }));
        var panel = Ui.V(0, Ui.Columns((head, Ui.Star), (nav, Ui.Auto)));
        ((FrameworkElement)panel.Children[0]).Margin = new Thickness(10, 4, 4, 4);
        if (stackExpanded)
        {
            panel.Children.Add(Ui.Line());
            for (var i = stack.Count - 1; i >= 0; i--)
            {
                var s = stack[i];
                var isCurrent = s.Number == current;
                var row = Ui.Columns(
                    (Ui.Text((i + 1).ToString(), 12, "dim"), new GridLength(18)),
                    (Ui.StateBadge(s.VisualState, true), Ui.Auto),
                    (Ui.Caption($"#{s.Number}"), Ui.Auto),
                    (Ui.Text(s.Title, 13, "text", isCurrent ? FontWeights.SemiBold : FontWeights.Normal), Ui.Star),
                    (BranchChip(s.HeadRefName, isCurrent ? "violet" : "slate"), Ui.Auto));
                var button = new Button
                {
                    Content = row,
                    HorizontalAlignment = HorizontalAlignment.Stretch,
                    HorizontalContentAlignment = HorizontalAlignment.Stretch,
                    Background = isCurrent ? Palette.Brush("indigo", 0.12) : new SolidColorBrush(Microsoft.UI.Colors.Transparent),
                    BorderThickness = new Thickness(0),
                    IsEnabled = !isCurrent
                };
                var number = s.Number;
                button.Click += (_, _) => app.Open(Route.Pr(new ItemRef(model.Ref.Repo, number)));
                panel.Children.Add(button);
            }
        }
        return new Border { Background = Palette.Brush("indigo", 0.06), CornerRadius = new CornerRadius(9), Child = panel };
    }

    private Button StackNav(StackPR s, string glyph, string help)
    {
        var b = new Button { Content = Ui.H(4, Ui.Glyph(glyph, 10), Ui.Text($"#{s.Number}", 12, "text", FontWeights.Medium)), Padding = new Thickness(8, 2, 8, 2), Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent), BorderThickness = new Thickness(0) };
        Ui.Tip(b, $"{help}: #{s.Number} {s.Title}");
        b.Click += (_, _) => app.Open(Route.Pr(new ItemRef(model.Ref.Repo, s.Number)));
        return b;
    }

    public static FrameworkElement? MergeStatusBar(PRDetailViewModel model, AppViewModel app, Action onMerge)
    {
        if (model.Pr is not { IsOpen: true } pr) return null;
        var checks = pr.Checks;
        var failed = checks.Count(c => c.Outcome == CheckOutcome.Failure);
        var pending = checks.Count(c => c.Outcome == CheckOutcome.Pending);
        var ok = checks.Count(c => c.Outcome == CheckOutcome.Success);
        var failedText = failed == 1 ? L.T("Merge_OneCheckFailed", "1 check failed") : L.F("Merge_ChecksFailed", "{0} checks failed", failed);
        var pendingText = pending == 1 ? L.T("Merge_OneCheckRunning", "1 check is still running") : L.F("Merge_ChecksRunning", "{0} checks are still running", pending);
        (string glyph, string color, string text, bool ready) s;
        if (pr.IsDraft) s = ("", "slate", L.T("Merge_Draft", "Draft — merging is unavailable"), false);
        else if (pr.Mergeable == "CONFLICTING" || pr.MergeStateStatus == "DIRTY") s = ("", "red", L.F("Merge_Conflicts", "Has conflicts with {0}", pr.BaseRefName), false);
        else
        {
            s = pr.MergeStateStatus switch
            {
                "BEHIND" => ("", "amber", L.F("Merge_Behind", "Branch is behind {0}", pr.BaseRefName), false),
                "BLOCKED" => pr.ReviewDecision switch
                {
                    "CHANGES_REQUESTED" => ("", "red", L.T("Review_ChangesRequested", "Changes requested"), false),
                    "REVIEW_REQUIRED" => ("", "amber", L.T("Review_Required", "Review required"), false),
                    _ => failed > 0 ? ("", "red", failedText, false) : pending > 0 ? ("", "amber", pendingText, false) : ("", "amber", L.T("Merge_Blocked", "Blocked by branch protection"), false)
                },
                "UNSTABLE" => failed > 0 ? ("", "amber", L.T("Merge_CanMergeBut", "Can be merged, but ") + failedText.ToLowerInvariant(), true) : pending > 0 ? ("", "amber", pendingText, true) : ("", "green", L.T("Merge_Ready", "Ready to merge"), true),
                "CLEAN" or "HAS_HOOKS" => ("", "green", L.T("Merge_Ready", "Ready to merge"), true),
                _ => ("", "slate", L.T("Merge_Checking", "Checking mergeability…"), false)
            };
        }
        var row = Ui.H(8, Ui.Glyph(s.glyph, 14, s.color), Ui.Text(s.text, 13));
        if (pr.AutoMergeRequest is { } auto)
        {
            var method = auto.MergeMethod is { } m ? $" ({MergeMethods.Title(m)})" : "";
            row.Children.Add(Ui.Text("· " + L.T("Merge_AutoEnabled", "auto-merge enabled") + method, 13, "green"));
        }
        if (checks.Count > 0)
        {
            var chips = Ui.H(6);
            if (failed > 0) chips.Children.Add(Ui.H(2, Ui.Glyph("", 11, "red"), Ui.Caption(failed.ToString(), "red")));
            if (pending > 0) chips.Children.Add(Ui.H(2, Ui.Glyph("", 11, "amber"), Ui.Caption(pending.ToString(), "amber")));
            chips.Children.Add(Ui.H(2, Ui.Glyph("", 11, "green"), Ui.Caption(ok.ToString(), "green")));
            var b = new Button { Content = chips, Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent), BorderThickness = new Thickness(0), Padding = new Thickness(4, 0, 4, 0) };
            Ui.Tip(b, L.T("PR_Checks", "Checks"));
            b.Click += (_, _) => model.Tab = PRTab.Checks;
            row.Children.Add(b);
        }
        var actions = Ui.H(6);
        if (pr.MergeStateStatus == "BEHIND" && (model.CanWrite || pr.ViewerCanUpdate))
            actions.Children.Add(Ui.Button(L.T("Merge_UpdateBranch", "Update branch"), () => _ = model.Simple(Q.UpdateBranch)));
        if (model.CanWrite && !pr.IsDraft)
        {
            var merge = Ui.Button(L.T("Merge_Merge", "Merge…"), onMerge, accent: s.ready);
            actions.Children.Add(merge);
        }
        var grid = Ui.Columns((row, Ui.Star), (actions, Ui.Auto));
        return new Border { Background = Palette.Brush(s.color, 0.08), CornerRadius = new CornerRadius(8), Padding = new Thickness(10, 6, 8, 6), Child = grid };
    }

    private void RenderBanners()
    {
        banners.Children.Clear();
        if (model.Pr is null) return;
        if (ReReviewBanner(model, () => _ = Dialogs.Reviewers(XamlRoot, model)) is { } rr) banners.Children.Add(rr);
        if (FixBanner(Pr, () => _ = Dialogs.FixWithClaude(XamlRoot, model)) is { } fix) banners.Children.Add(fix);
        banners.Visibility = banners.Children.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    public static FrameworkElement? ReReviewBanner(PRDetailViewModel model, Action more)
    {
        var pr = model.Pr!;
        var stale = model.StaleReviews;
        if (!pr.IsOpen || !(pr.ViewerDidAuthor || pr.ViewerCanUpdate) || stale.Count == 0) return null;
        var logins = stale.Select(s => s.Login).ToList();
        var who = stale.Count == 1 ? $"@{stale[0].Login}" : string.Join(", ", stale.SkipLast(1).Select(s => "@" + s.Login)) + $" and @{stale[^1].Login}";
        var counts = stale.Where(s => s.Commits is not null).Select(s => s.Commits!.Value).ToList();
        var what = counts.Count == stale.Count && counts.Count > 0 ? Fmt.Plural(counts.Max(), "commit", "commits") : "new commits";
        var message = pr.ViewerDidAuthor ? $"You pushed {what} after {who} reviewed" : $"{char.ToUpperInvariant(what[0])}{what[1..]} pushed after {who} reviewed";
        var row = Ui.H(8, Ui.IconTile("", "sky", 18), Ui.Text(message, 13));
        if (model.LastCommitDate is { } d) row.Children.Add(Ui.Caption("· " + Fmt.Rel(d)));
        var other = Ui.Button(L.T("ReReview_Other", "Other…"), more);
        var send = Ui.Button(logins.Count == 1 ? L.T("ReReview_Request", "Re-request review") : L.F("ReReview_RequestN", "Re-request review ({0})", logins.Count), () => { }, accent: true, glyph: "");
        send.Click += async (_, _) =>
        {
            send.IsEnabled = false;
            if (await model.RequestReviewers(logins)) send.Content = L.T("ReReview_Requested", "Requested");
            else send.IsEnabled = true;
        };
        var grid = Ui.Columns((row, Ui.Star), (Ui.H(6, other, send), Ui.Auto));
        return new Border { Background = Palette.Brush("sky", 0.08), CornerRadius = new CornerRadius(10), Padding = new Thickness(10, 6, 8, 6), Child = grid };
    }

    public static FrameworkElement? FixBanner(PullRequestDetail pr, Action action)
    {
        var open = pr.ReviewThreads.Items.Where(t => !t.IsResolved).ToList();
        var unresolved = open.Count(t => !t.IsOutdated);
        var outdated = open.Count - unresolved;
        var lastCommit = pr.LastCommit.Items.FirstOrDefault()?.Commit.CommittedDate;
        var found = FixPromptExtractor.Latest(pr);
        if (found is { Date: { } fd } && lastCommit is { } lc && lc > fd && unresolved == 0) found = null;
        if (!pr.IsOpen || (unresolved == 0 && found is null)) return null;
        var row = Ui.H(8, Ui.IconTile("", "coral", 18));
        if (unresolved > 0) row.Children.Add(Ui.Text(Fmt.Plural(unresolved, "unresolved comment", "unresolved comments"), 13));
        if (outdated > 0)
        {
            var o = Ui.Text($"· {outdated} outdated", 13, "dim");
            Ui.Tip(o, L.T("Fix_OutdatedHelp", "Code these comments point to has changed since they were written"));
            row.Children.Add(o);
        }
        if (found is not null)
        {
            if (unresolved > 0) row.Children.Add(Ui.Text("·", 13, "dim"));
            row.Children.Add(Ui.Text(L.F("Fix_PromptBy", "Fix prompt by @{0}", found.Author) + (found.Date is { } d ? " · " + Fmt.Rel(d) : ""), 13, "muted"));
        }
        var button = Ui.Button(L.T("Fix_Button", "Fix with Claude Code"), action, accent: true);
        button.Background = Palette.Brush("coral");
        button.Foreground = new SolidColorBrush(Microsoft.UI.Colors.White);
        var grid = Ui.Columns((row, Ui.Star), (button, Ui.Auto));
        return new Border { Background = Palette.Brush("coral", 0.08), CornerRadius = new CornerRadius(10), Padding = new Thickness(10, 6, 8, 6), Child = grid };
    }

    private void RenderToolbar()
    {
        if (model.Pr is null) return;
        var pr = Pr;
        toolbar.Children.Clear();
        if (pr.IsOpen)
        {
            var review = new SplitButton
            {
                Content = Ui.H(6, Ui.Glyph("", 13, "text"), new TextBlock { Text = model.PendingCount == 0 ? L.T("PR_Review", "Review") : L.F("PR_ReviewN", "Review ({0})", model.PendingCount) })
            };
            review.Click += (_, _) => _ = Dialogs.Review(XamlRoot, model);
            var flyout = Ui.Menu(Ui.MenuItem(L.T("PR_WriteReview", "Write review…"), () => _ = Dialogs.Review(XamlRoot, model)));
            if (!pr.ViewerDidAuthor) flyout.Items.Add(Ui.MenuItem(L.T("PR_Approve", "Approve"), () => _ = ConfirmApprove()));
            review.Flyout = flyout;
            Ui.Tip(review, L.T("PR_SubmitReview", "Submit review"));
            toolbar.Children.Add(review);
            toolbar.Children.Add(Ui.IconButton("", L.T("PR_RequestReview", "Request or re-request review"), () => _ = Dialogs.Reviewers(XamlRoot, model), "text"));
        }
        toolbar.Children.Add(Ui.IconButton("", L.T("PR_FixWithClaude", "Send review comments to a new Claude Code session"), () => _ = Dialogs.FixWithClaude(XamlRoot, model), "coral"));
        if (pr.IsOpen && !pr.IsDraft && model.CanWrite)
            toolbar.Children.Add(Ui.IconButton("", L.T("PR_Merge", "Merge pull request"), () => _ = Dialogs.Merge(XamlRoot, model), "green"));
        var more = Ui.IconButton("", L.T("Common_More", "More"), () => { }, "text");
        more.Flyout = MoreMenu();
        toolbar.Children.Add(more);
        toolbar.Children.Add(Ui.IconButton("", L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(pr.Url), "text"));
    }

    private MenuFlyout MoreMenu()
    {
        var pr = Pr;
        var menu = Ui.Menu(
            Ui.MenuItem(L.T("Menu_CopyLink", "Copy Link"), () => WindowInterop.Copy(pr.Url), ""),
            Ui.MenuItem(L.F("PR_CopyCheckout", "Copy “gh pr checkout {0}”", pr.Number), () => WindowInterop.Copy($"gh pr checkout {pr.Number} -R {model.Ref.Repo.Full}"), ""),
            Ui.MenuItem(L.T("PR_CopyBranch", "Copy branch name"), () => WindowInterop.Copy(pr.HeadRefName), ""),
            Ui.Separator());
        if (pr.ViewerCanUpdate) menu.Items.Add(Ui.MenuItem(L.T("PR_Edit", "Edit title and description…"), () => _ = Dialogs.EditPR(XamlRoot, model), ""));
        if (pr.IsOpen)
        {
            menu.Items.Add(Ui.MenuItem(L.T("PR_RequestReviewMenu", "Request review…"), () => _ = Dialogs.Reviewers(XamlRoot, model)));
            if (pr.ViewerCanUpdate)
            {
                menu.Items.Add(pr.IsDraft
                    ? Ui.MenuItem(L.T("PR_ReadyForReview", "Ready for review"), () => _ = model.Simple(Q.ReadyForReview))
                    : Ui.MenuItem(L.T("PR_ConvertToDraft", "Convert to draft"), () => _ = model.Simple(Q.ConvertToDraft)));
            }
            if (pr.MergeStateStatus is "BEHIND" or "BLOCKED" or "UNSTABLE")
                menu.Items.Add(Ui.MenuItem(L.F("PR_UpdateBranchFrom", "Update branch from {0}", pr.BaseRefName), () => _ = model.Simple(Q.UpdateBranch)));
            if (pr.AutoMergeRequest is not null)
                menu.Items.Add(Ui.MenuItem(L.T("PR_DisableAutoMerge", "Disable auto-merge"), () => _ = Confirm(L.T("PR_DisableAutoMergeQ", "Disable auto-merge?"), L.T("PR_Disable", "Disable"), () => model.Simple(Q.DisableAutoMerge))));
            menu.Items.Add(Ui.Separator());
            if (pr.ViewerCanUpdate)
                menu.Items.Add(Ui.MenuItem(L.T("PR_Close", "Close pull request…"), () => _ = Confirm(L.T("PR_CloseQ", "Close pull request without merging?"), L.T("PR_CloseButton", "Close pull request"), () => model.Simple(Q.ClosePR))));
        }
        else if (!pr.Merged && pr.ViewerCanUpdate)
        {
            menu.Items.Add(Ui.MenuItem(L.T("PR_Reopen", "Reopen pull request"), () => _ = model.Simple(Q.ReopenPR)));
        }
        if (!pr.IsOpen && pr.HeadRef is not null && model.CanWrite)
            menu.Items.Add(Ui.MenuItem(L.F("PR_DeleteBranch", "Delete branch {0}…", pr.HeadRefName), () => _ = Confirm(L.F("PR_DeleteBranchQ", "Delete branch {0}?", pr.HeadRefName), L.T("PR_DeleteBranchButton", "Delete branch"), model.DeleteBranch)));
        return menu;
    }

    private async Task ConfirmApprove()
    {
        if (await Dialogs.Confirm(XamlRoot, L.T("PR_ApproveQ", "Approve this pull request?"), L.T("PR_Approve", "Approve"), destructive: false))
            await model.SubmitReview("APPROVE", "");
    }

    private async Task Confirm(string title, string button, Func<Task<bool>> action)
    {
        if (await Dialogs.Confirm(XamlRoot, title, button)) await action();
    }

    private void RenderTabs()
    {
        var pr = Pr;
        var unresolved = pr.ReviewThreads.Items.Count(t => !t.IsResolved && !t.IsOutdated);
        var checks = pr.Checks;
        var failed = checks.Count(c => c.Outcome == CheckOutcome.Failure);
        var pending = checks.Count(c => c.Outcome == CheckOutcome.Pending);
        var summary = checks.Count == 0 ? "0" : failed > 0 ? $"✕{failed}" : pending > 0 ? $"…{pending}" : $"✓{checks.Count}";
        var color = failed > 0 ? "red" : pending > 0 ? "amber" : checks.Count == 0 ? "muted" : "green";
        tabs.SetTabs(new[]
        {
            new TabSpec(nameof(PRTab.Conversation), L.T("PR_TabConversation", "Conversation"), "", unresolved > 0 ? unresolved.ToString() : null),
            new TabSpec(nameof(PRTab.Commits), L.T("PR_TabCommits", "Commits"), "", (pr.AllCommits.TotalCount ?? pr.AllCommits.Items.Count).ToString()),
            new TabSpec(nameof(PRTab.Checks), L.T("PR_TabChecks", "Checks"), "", summary, color),
            new TabSpec(nameof(PRTab.Files), L.T("PR_TabFiles", "Files changed"), "", pr.ChangedFiles.ToString())
        });
        tabs.Selected = model.Tab.ToString();
    }

    private void ShowTab()
    {
        tabs.Selected = model.Tab.ToString();
        if (!tabViews.ContainsKey(model.Tab))
        {
            var view = model.Tab switch
            {
                PRTab.Conversation => BuildConversation(),
                PRTab.Commits => BuildCommits(null),
                PRTab.Checks => BuildChecks(null),
                _ => BuildFiles()
            };
            tabViews[model.Tab] = view;
            tabArea.Children.Add(view);
        }
        foreach (var (tab, view) in tabViews) view.Visibility = tab == model.Tab ? Visibility.Visible : Visibility.Collapsed;
        if (model.Tab == PRTab.Files) _ = model.LoadFiles();
    }

    private FrameworkElement BuildConversation()
    {
        var grid = new Grid();
        grid.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        conversation = new HtmlView();
        conversation.LinkClicked += url => HtmlView.OpenLink(url);
        conversation.ActionInvoked += OnConversationAction;
        grid.Children.Add(conversation);
        var composer = Composer(L.T("PR_CommentPlaceholder", "Comment (Markdown) — Ctrl+Enter to send"), L.T("PR_Comment", "Comment"), model.Comment);
        SetRow(composer, 1);
        grid.Children.Add(composer);
        RenderConversation();
        return grid;
    }

    public static FrameworkElement Composer(string placeholder, string buttonText, Func<string, Task<bool>> send)
    {
        var box = new TextBox { PlaceholderText = placeholder, AcceptsReturn = true, TextWrapping = TextWrapping.Wrap, MinHeight = 64, MaxHeight = 200 };
        ScrollViewer.SetVerticalScrollBarVisibility(box, ScrollBarVisibility.Auto);
        var button = Ui.Button(buttonText, () => { }, accent: true);
        button.VerticalAlignment = VerticalAlignment.Bottom;
        button.IsEnabled = false;
        box.TextChanged += (_, _) => button.IsEnabled = box.Text.Trim().Length > 0;
        async Task Submit()
        {
            var body = box.Text;
            if (body.Trim().Length == 0) return;
            button.IsEnabled = false;
            if (await send(body)) box.Text = "";
            button.IsEnabled = box.Text.Trim().Length > 0;
        }
        button.Click += (_, _) => _ = Submit();
        var acc = new KeyboardAccelerator { Key = VirtualKey.Enter, Modifiers = VirtualKeyModifiers.Control };
        acc.Invoked += (_, e) =>
        {
            e.Handled = true;
            _ = Submit();
        };
        box.KeyboardAccelerators.Add(acc);
        var grid = Ui.Columns((box, Ui.Star), (button, Ui.Auto));
        grid.Padding = new Thickness(10);
        var wrapper = new Grid();
        var line = Ui.Line();
        line.VerticalAlignment = VerticalAlignment.Top;
        wrapper.Children.Add(line);
        wrapper.Children.Add(grid);
        return wrapper;
    }

    private void RenderConversation()
    {
        if (conversation is null || model.Html.Length == 0 || model.Html == lastHtml) return;
        var first = lastHtml.Length == 0;
        lastHtml = model.Html;
        conversation.Render(model.Html, "conversation", first ? "top" : null);
        if (model.ScrollToBottomToken != lastScrollToken)
        {
            lastScrollToken = model.ScrollToBottomToken;
            conversation.ScrollToBottom();
        }
    }

    private void OnConversationAction(string type, string id)
    {
        switch (type)
        {
            case "reply":
                if (Pr.ReviewThreads.Items.FirstOrDefault(t => t.Id == id) is { } thread) _ = Dialogs.Reply(XamlRoot, model, thread);
                break;
            case "resolve":
                _ = model.SetResolved(id, true);
                break;
            case "unresolve":
                _ = model.SetResolved(id, false);
                break;
            case "file":
                model.RevealFile(id);
                _ = model.LoadFiles();
                break;
        }
    }

    private FrameworkElement BuildCommits(FrameworkElement? existing)
    {
        var list = existing as ListView ?? ListSupport.Create("GenericRowTemplate");
        if (existing is null)
        {
            list.ItemClick += (_, e) =>
            {
                if (e.ClickedItem is CommitRowItem c) app.Open(Route.Commit(model.Ref.Repo, c.Commit.Oid), true);
            };
            list.SelectionMode = ListViewSelectionMode.None;
            list.ContextRequested += (_, args) =>
            {
                if ((args.OriginalSource as FrameworkElement)?.DataContext is not CommitRowItem c) return;
                args.Handled = true;
                Ui.Menu(
                    Ui.MenuItem(L.T("Commit_CopySha", "Copy SHA"), () => WindowInterop.Copy(c.Commit.Oid)),
                    Ui.MenuItem(L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(c.Commit.Url))).ShowAt((FrameworkElement)args.OriginalSource);
            };
        }
        if (model.Pr is { } pr)
        {
            var items = pr.AllCommits.Items.AsEnumerable().Reverse().Select(c => (object)new CommitRowItem(c.Commit)).ToList();
            if ((pr.AllCommits.TotalCount ?? 0) > pr.AllCommits.Items.Count)
                items.Add(new ActionRowItem(L.F("PR_AllCommits", "Show all {0} commits in browser", pr.AllCommits.TotalCount ?? 0), () => LinkBrowser.Open(pr.Url + "/commits")));
            list.ItemsSource = items;
        }
        return list;
    }

    private FrameworkElement BuildChecks(FrameworkElement? existing)
    {
        var list = existing as ListView ?? ListSupport.Create("GenericRowTemplate", ListViewSelectionMode.None);
        if (existing is null)
        {
            list.ItemClick += (_, e) =>
            {
                if (e.ClickedItem is ActionRowItem a) a.Action();
            };
        }
        if (model.Pr is { } pr)
        {
            var checks = pr.Checks.OrderBy(c => (int)c.Outcome).ThenBy(c => c.Title).ToList();
            var failedRuns = checks.Where(c => c.Outcome == CheckOutcome.Failure && c.RunId is not null).Select(c => c.RunId).Distinct().Count();
            var items = new List<object>();
            if (checks.Count == 0) items.Add(new TextRowItem(L.T("Checks_None", "No checks for the latest commit")));
            if (failedRuns > 0 && model.CanWrite) items.Add(new ActionRowItem(L.F("Checks_Rerun", "Re-run failed jobs ({0} workflow)", failedRuns), () => _ = model.RerunFailed(), ""));
            foreach (var c in checks) items.Add(new CheckRowItem(c, () => _ = Dialogs.JobLog(XamlRoot, model.Api, model.Ref.Repo, c)));
            list.ItemsSource = items;
        }
        return list;
    }

    private FrameworkElement BuildFiles()
    {
        var grid = new Grid();
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(270) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

        var side = new Grid { Background = ThemeService.IsBrand ? Palette.Brush("list") : null };
        side.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        side.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        fileFilter = new TextBox { PlaceholderText = L.T("Files_Filter", "Filter files") };
        viewedCount = Ui.Caption("");
        Ui.Tip(viewedCount, L.T("Files_Viewed", "Viewed"));
        DispatcherTimer? debounce = null;
        fileFilter.TextChanged += (_, _) =>
        {
            debounce?.Stop();
            debounce = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(250) };
            debounce.Tick += (t, _) =>
            {
                ((DispatcherTimer)t!).Stop();
                RenderFiles();
            };
            debounce.Start();
        };
        var filterRow = Ui.Columns((fileFilter, Ui.Star), (viewedCount, Ui.Auto));
        filterRow.Padding = new Thickness(8);
        side.Children.Add(filterRow);
        fileList = ListSupport.Create("GenericRowTemplate");
        fileList.SelectionChanged += (_, _) =>
        {
            if (fileList.SelectedItem is FileListItem f && model.ScrollTarget != f.File.Id) model.RevealFile(f.File.Id);
        };
        SetRow(fileList, 1);
        side.Children.Add(fileList);
        grid.Children.Add(side);

        var line = Ui.Line(vertical: true);
        SetColumn(line, 1);
        grid.Children.Add(line);

        var right = new Grid();
        diff = new HtmlView();
        diff.LinkClicked += url => HtmlView.OpenLink(url);
        diff.ActionInvoked += OnDiffAction;
        right.Children.Add(diff);
        SetColumn(right, 2);
        grid.Children.Add(right);
        renderedDiffVersion = -1;
        RenderFiles();
        return grid;
    }

    private void RenderFiles()
    {
        if (diff is null || fileList is null) return;
        if (!model.FilesLoaded)
        {
            if (model.FilesError is { } e) diff.Render($"<div class=\"tl\"><p class=\"empty\">{ConversationHtml.Esc(e)}</p></div>");
            return;
        }
        var filter = fileFilter?.Text.Trim() ?? "";
        var visible = model.Files.Where(f => filter.Length == 0 || f.Id.Contains(filter, StringComparison.OrdinalIgnoreCase)).ToList();
        var selected = model.ScrollTarget;
        fileList.ItemsSource = visible.Select(f => new FileListItem(f, model.ViewedPaths.Contains(f.Id), model.ThreadCount(f.Id))).ToList();
        if (selected is not null) fileList.SelectedItem = (fileList.ItemsSource as List<FileListItem>)?.FirstOrDefault(f => f.File.Id == selected);
        if (viewedCount is not null) viewedCount.Text = $"{model.ViewedPaths.Intersect(model.Files.Select(f => f.Id)).Count()}/{model.Files.Count}";
        if (renderedDiffVersion == model.DiffVersion && renderedFilter == filter) return;
        var first = renderedDiffVersion < 0;
        renderedDiffVersion = model.DiffVersion;
        renderedFilter = filter;
        var html = DiffHtml.Render(model.Files, model.RenderOptions(filter));
        diff.Render(html, "diff", first && selected is not null ? "file:" + selected : first ? "top" : null, "flush");
    }

    private void OnDiffAction(string type, string id)
    {
        switch (type)
        {
            case "comment":
            {
                var parts = id.Split('|');
                if (parts.Length >= 3 && int.TryParse(parts[^1], out var line))
                {
                    var path = string.Join("|", parts[..^2]);
                    _ = Dialogs.InlineComment(XamlRoot, model, new DiffAnchor(path, line, parts[^2]));
                }
                break;
            }
            case "viewed":
                _ = model.ToggleViewed(id);
                break;
            case "copy":
                WindowInterop.Copy(id);
                break;
            case "collapse":
                model.Collapsed.Add(id);
                break;
            case "expand":
                model.Collapsed.Remove(id);
                if (model.Files.FirstOrDefault(f => f.Id == id) is { } file)
                    diff?.Send(new JsonObject { ["op"] = "fileBody", ["path"] = id, ["html"] = DiffHtml.Body(file, model.RenderOptions("")) });
                break;
            case "deletePending":
                if (Guid.TryParse(id, out var gid)) model.RemovePending(gid);
                break;
            case "reply":
                if (Pr.ReviewThreads.Items.FirstOrDefault(t => t.Id == id) is { } thread) _ = Dialogs.Reply(XamlRoot, model, thread);
                break;
            case "resolve":
                _ = model.SetResolved(id, true);
                break;
            case "unresolve":
                _ = model.SetResolved(id, false);
                break;
        }
    }
}

public sealed record TextRowItem(string Text) : IRowContent
{
    public UIElement BuildRow()
    {
        var t = Ui.Text(Text, 13, "muted");
        t.Margin = new Thickness(0, 8, 0, 8);
        return t;
    }
}

public sealed record ActionRowItem(string Text, Action Action, string Glyph = "") : IRowContent
{
    public UIElement BuildRow()
    {
        var p = Ui.H(8, Ui.Glyph(Glyph, 12, "accent"), Ui.Text(Text, 13, "accent"));
        p.Margin = new Thickness(0, 8, 0, 8);
        return p;
    }
}

public sealed record CommitRowItem(CommitInfo Commit) : IRowContent
{
    public UIElement BuildRow()
    {
        var c = Commit;
        var body = Ui.V(2, Ui.Text(c.MessageHeadline, 13, maxLines: 2), Ui.Caption($"{c.Author?.User?.Login ?? c.Author?.Name ?? ""} · {Fmt.Rel(c.CommittedDate)}"));
        var right = Ui.H(8);
        if (Ui.CiIcon(c.StatusCheckRollup?.State) is { } ci) right.Children.Add(ci);
        right.Children.Add(Ui.Text(c.AbbreviatedOid, 12, "muted", mono: true));
        var grid = Ui.Columns((Ui.Avatar(c.Author?.AvatarUrl, 22), Ui.Auto), (body, Ui.Star), (right, Ui.Auto));
        grid.ColumnSpacing = 10;
        grid.Padding = new Thickness(0, 6, 0, 6);
        return grid;
    }
}

public sealed record CheckRowItem(CheckContext Check, Action ShowLog) : IRowContent
{
    public UIElement BuildRow()
    {
        var c = Check;
        var parts = new List<string>
        {
            c.Outcome switch
            {
                CheckOutcome.Success => "Successful",
                CheckOutcome.Failure => "Failed",
                CheckOutcome.Pending => c.Status == "QUEUED" ? "Queued" : "In progress",
                CheckOutcome.Neutral => c.Conclusion == "CANCELLED" ? "Cancelled" : "Neutral",
                _ => "Skipped"
            }
        };
        if (c.Duration is { } d) parts.Add(d);
        if (!string.IsNullOrEmpty(c.Description)) parts.Add(c.Description);
        var body = Ui.V(1, Ui.Text(c.FullTitle, 13), Ui.Caption(string.Join(" · ", parts)));
        var right = Ui.H(4);
        if (c.IsActionsJob && c.Outcome is not CheckOutcome.Pending and not CheckOutcome.Skipped)
            right.Children.Add(Ui.Button(L.T("Checks_Log", "Log"), ShowLog));
        if (c.Link is { } link) right.Children.Add(Ui.IconButton("", L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(link)));
        var grid = Ui.Columns((Ui.Glyph(Palette.CheckGlyph(c.Outcome), 15, Palette.CheckKey(c.Outcome)), Ui.Auto), (body, Ui.Star), (right, Ui.Auto));
        grid.ColumnSpacing = 10;
        grid.Padding = new Thickness(0, 4, 0, 4);
        return grid;
    }
}

public sealed record FileListItem(DiffFile File, bool Viewed, int Threads) : IRowContent
{
    public UIElement BuildRow()
    {
        var color = File.Status switch
        {
            "added" => "green",
            "removed" => "red",
            "renamed" => "violet",
            _ => "amber"
        };
        var letter = Ui.Text(File.StatusLetter, 11, color, FontWeights.Bold, mono: true);
        letter.Width = 12;
        var dir = Ui.Text(File.Directory, 11, "muted");
        dir.TextTrimming = TextTrimming.CharacterEllipsis;
        var right = Ui.H(4);
        if (Threads > 0) right.Children.Add(Ui.Glyph("", 11));
        if (Viewed) right.Children.Add(Ui.Glyph("", 12, "green"));
        var grid = Ui.Columns((letter, Ui.Auto), (Ui.V(0, Ui.Text(File.FileName, 13), dir), Ui.Star), (right, Ui.Auto));
        grid.ColumnSpacing = 6;
        grid.Padding = new Thickness(0, 3, 0, 3);
        Ui.Tip(grid, File.Id);
        return grid;
    }
}

public static class MergeMethods
{
    public static readonly string[] All = { "MERGE", "SQUASH", "REBASE" };

    public static string Title(string m) => m switch
    {
        "SQUASH" => L.T("Merge_Squash", "Squash and merge"),
        "REBASE" => L.T("Merge_Rebase", "Rebase and merge"),
        _ => L.T("Merge_Commit", "Merge commit")
    };
}
