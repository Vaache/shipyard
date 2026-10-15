using System.Text;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Shipyard.Controls;
using Shipyard.Core.Api;
using Shipyard.Core.Claude;
using Shipyard.Core.Git;
using Shipyard.Core.Html;
using Shipyard.Core.Util;
using Shipyard.Services;
using Shipyard.ViewModels;

namespace Shipyard.Views;

public static class Dialogs
{
    private static bool open;

    public static ContentDialog Create(XamlRoot root, string title, UIElement content, string? primary = null, string? secondary = null, string close = "Cancel", double maxWidth = 620)
    {
        var d = new ContentDialog
        {
            XamlRoot = root,
            Title = title,
            Content = content,
            CloseButtonText = close == "Cancel" ? L.T("Common_Cancel", "Cancel") : close,
            DefaultButton = primary is not null ? ContentDialogButton.Primary : ContentDialogButton.Close,
            RequestedTheme = (root.Content as FrameworkElement)?.ActualTheme ?? ElementTheme.Default
        };
        if (primary is not null) d.PrimaryButtonText = primary;
        if (secondary is not null) d.SecondaryButtonText = secondary;
        d.Resources["ContentDialogMaxWidth"] = maxWidth;
        d.Resources["ContentDialogMinWidth"] = Math.Min(maxWidth, 460.0);
        return d;
    }

    public static async Task<ContentDialogResult> ShowAsync(ContentDialog d)
    {
        if (open) return ContentDialogResult.None;
        open = true;
        try
        {
            return await d.ShowAsync();
        }
        catch
        {
            return ContentDialogResult.None;
        }
        finally
        {
            open = false;
        }
    }

    public static async Task Alert(XamlRoot? root, string title, string message)
    {
        if (root is null) return;
        var d = Create(root, title, Ui.Text(message, 13, "text", wrap: true, select: true), close: L.T("Common_OK", "OK"));
        await ShowAsync(d);
    }

    public static async Task<bool> Confirm(XamlRoot? root, string title, string button, bool destructive = true, string? message = null)
    {
        if (root is null) return false;
        var d = Create(root, title, message is null ? new Grid() : Ui.Text(message, 13, "muted", wrap: true), button);
        if (destructive) d.PrimaryButtonStyle = (Style)Application.Current.Resources["AccentButtonStyle"];
        return await ShowAsync(d) == ContentDialogResult.Primary;
    }

    public static TextBox Editor(string placeholder, string text = "", double minHeight = 120, bool mono = false)
    {
        var box = new TextBox
        {
            PlaceholderText = placeholder,
            Text = text,
            AcceptsReturn = true,
            TextWrapping = TextWrapping.Wrap,
            MinHeight = minHeight,
            MaxHeight = 420
        };
        if (mono) box.FontFamily = Ui.Mono;
        ScrollViewer.SetVerticalScrollBarVisibility(box, ScrollBarVisibility.Auto);
        return box;
    }

    private static async Task<bool> RunWhileOpen(ContentDialog d, Func<Task<bool>> action, ContentDialogButtonClickEventArgs args)
    {
        var deferral = args.GetDeferral();
        try
        {
            var ok = await action();
            args.Cancel = !ok;
            return ok;
        }
        finally
        {
            deferral.Complete();
        }
    }

    public static async Task Review(XamlRoot? root, PRDetailViewModel model)
    {
        if (root is null || model.Pr is not { } pr) return;
        var panel = Ui.V(10);
        if (model.PendingCount > 0)
            panel.Children.Add(Ui.H(6, Ui.Glyph("", 12), Ui.Text(L.F("Review_PendingInline", "Inline comments to be submitted: {0}", model.PendingCount), 13, "muted")));
        var text = Editor(L.T("Review_Summary", "Review summary (optional when approving)"), minHeight: 140);
        panel.Children.Add(text);
        var radios = new RadioButtons();
        var comment = new RadioButton { Content = L.T("Review_CommentOnly", "Comment — without approval"), Tag = "COMMENT", IsChecked = true };
        var approve = new RadioButton { Content = L.T("PR_Approve", "Approve"), Tag = "APPROVE", IsEnabled = !pr.ViewerDidAuthor };
        var changes = new RadioButton { Content = L.T("Review_RequestChanges", "Request changes"), Tag = "REQUEST_CHANGES", IsEnabled = !pr.ViewerDidAuthor };
        radios.Items.Add(comment);
        radios.Items.Add(approve);
        radios.Items.Add(changes);
        radios.SelectedIndex = 0;
        panel.Children.Add(radios);
        var d = Create(root, L.F("Review_Title", "Review #{0}", pr.Number), panel, L.T("Review_Submit", "Submit review"), model.PendingCount > 0 ? L.T("Review_Discard", "Discard pending comments") : null);
        string Event() => (radios.SelectedItem as RadioButton)?.Tag as string ?? "COMMENT";
        void Validate() => d.IsPrimaryButtonEnabled = Event() == "APPROVE" || text.Text.Trim().Length > 0 || model.PendingCount > 0;
        text.TextChanged += (_, _) => Validate();
        radios.SelectionChanged += (_, _) => Validate();
        Validate();
        d.PrimaryButtonClick += async (_, args) => await RunWhileOpen(d, () => model.SubmitReview(Event(), text.Text), args);
        d.SecondaryButtonClick += (_, _) => model.ClearPending();
        await ShowAsync(d);
    }

    public static async Task Merge(XamlRoot? root, PRDetailViewModel model)
    {
        if (root is null || model.Pr is not { } pr) return;
        var repo = model.Repo;
        var allowed = MergeMethods.All.Where(m => repo is null || m switch
        {
            "MERGE" => repo.MergeCommitAllowed,
            "SQUASH" => repo.SquashMergeAllowed,
            _ => repo.RebaseMergeAllowed
        }).ToList();
        var methods = new ComboBox { HorizontalAlignment = HorizontalAlignment.Stretch, Header = L.T("Merge_Method", "Method") };
        foreach (var m in allowed) methods.Items.Add(new ComboBoxItem { Content = MergeMethods.Title(m), Tag = m });
        var def = repo?.ViewerDefaultMergeMethod ?? "MERGE";
        methods.SelectedIndex = Math.Max(0, allowed.IndexOf(allowed.Contains(def) ? def : allowed.FirstOrDefault() ?? "MERGE"));
        var title = new TextBox { PlaceholderText = L.T("Merge_TitlePlaceholder", "Commit title (GitHub default)") };
        var message = Editor(L.T("Merge_BodyPlaceholder", "Commit description (optional)"), minHeight: 80);
        var sameRepo = pr.HeadRepository?.NameWithOwner == model.Ref.Repo.Full;
        var deleteBox = new CheckBox { Content = L.F("Merge_DeleteBranch", "Delete branch {0} after merge", pr.HeadRefName), IsChecked = true, Visibility = !(repo?.DeleteBranchOnMerge ?? false) && sameRepo ? Visibility.Visible : Visibility.Collapsed };
        string Method() => (methods.SelectedItem as ComboBoxItem)?.Tag as string ?? "MERGE";
        void Prefill()
        {
            title.Text = Method() == "SQUASH" ? $"{pr.Title} (#{pr.Number})" : "";
            var rebase = Method() == "REBASE";
            title.Visibility = rebase ? Visibility.Collapsed : Visibility.Visible;
            message.Visibility = rebase ? Visibility.Collapsed : Visibility.Visible;
        }
        methods.SelectionChanged += (_, _) => Prefill();
        Prefill();
        var canAuto = repo?.AutoMergeAllowed == true && pr.AutoMergeRequest is null && pr.MergeStateStatus != "CLEAN";
        var d = Create(root, L.F("Merge_Title", "Merge #{0} into {1}", pr.Number, pr.BaseRefName), Ui.V(10, methods, title, message, deleteBox), L.T("Merge_MergeButton", "Merge"), canAuto ? L.T("Merge_EnableAuto", "Enable auto-merge") : null);
        d.IsPrimaryButtonEnabled = allowed.Count > 0;
        d.PrimaryButtonClick += async (_, args) =>
        {
            var t = title.Text.Trim();
            var b = message.Text.Trim();
            var del = deleteBox.IsChecked == true && deleteBox.Visibility == Visibility.Visible;
            await RunWhileOpen(d, () => model.Merge(Method(), t.Length == 0 ? null : t, b.Length == 0 ? null : b, del), args);
        };
        d.SecondaryButtonClick += async (_, args) => await RunWhileOpen(d, () => model.EnableAutoMerge(Method()), args);
        await ShowAsync(d);
    }

    public static async Task EditPR(XamlRoot? root, PRDetailViewModel model)
    {
        if (root is null || model.Pr is not { } pr) return;
        var title = new TextBox { Header = L.T("Edit_Title", "Title"), Text = pr.Title };
        var body = Editor(L.T("Edit_Body", "Description (Markdown)"), pr.Body, 260);
        var d = Create(root, L.F("Edit_Header", "Edit #{0}", pr.Number), Ui.V(10, title, body), L.T("Common_Save", "Save"), maxWidth: 760);
        title.TextChanged += (_, _) => d.IsPrimaryButtonEnabled = title.Text.Trim().Length > 0;
        d.PrimaryButtonClick += async (_, args) => await RunWhileOpen(d, () => model.Edit(title.Text, body.Text), args);
        await ShowAsync(d);
    }

    public static async Task Reviewers(XamlRoot? root, PRDetailViewModel model)
    {
        if (root is null) return;
        var candidates = model.Reviewers.Where(r => r.Login != model.ViewerLogin && !r.Login.EndsWith("[bot]") && !r.Login.Contains('/')).ToList();
        var stale = model.ReviewersToReRequest;
        var checks = new List<(CheckBox Box, string Login)>();
        var panel = Ui.V(8);
        if (candidates.Count > 0)
        {
            panel.Children.Add(Ui.Caption(L.T("Reviewers_OnPR", "Reviewers on this pull request")));
            foreach (var r in candidates)
            {
                var content = Ui.H(6, PRContent.ReviewerBadge(r), Ui.Text(r.Login, 13));
                if (stale.Contains(r.Login)) content.Children.Add(Ui.Caption(L.T("Reviewers_Stale", "reviewed before latest push"), "amber"));
                else if (r.State == "PENDING") content.Children.Add(Ui.Caption(L.T("Reviewers_Already", "already requested")));
                var box = new CheckBox { Content = content, IsChecked = stale.Contains(r.Login) };
                checks.Add((box, r.Login));
                panel.Children.Add(box);
            }
        }
        var typed = new TextBox { PlaceholderText = L.T("Reviewers_Other", "Other logins, comma-separated, e.g. alice, bob") };
        panel.Children.Add(typed);
        List<string> Requested()
        {
            var seen = new HashSet<string>();
            var list = checks.Where(c => c.Box.IsChecked == true).Select(c => c.Login)
                .Concat(typed.Text.Split(new[] { ',', ' ' }, StringSplitOptions.RemoveEmptyEntries).Select(s => s.Trim('@', ' ')).Where(s => s.Length > 0));
            return list.Where(seen.Add).ToList();
        }
        var d = Create(root, L.T("Reviewers_Title", "Request review"), panel, L.T("Reviewers_Request", "Request"));
        void Validate()
        {
            var n = Requested().Count;
            d.IsPrimaryButtonEnabled = n > 0;
            d.PrimaryButtonText = n == 0 ? L.T("Reviewers_Request", "Request") : L.F("Reviewers_RequestN", "Request ({0})", n);
        }
        foreach (var (box, _) in checks)
        {
            box.Checked += (_, _) => Validate();
            box.Unchecked += (_, _) => Validate();
        }
        typed.TextChanged += (_, _) => Validate();
        Validate();
        d.PrimaryButtonClick += async (_, args) => await RunWhileOpen(d, () => model.RequestReviewers(Requested()), args);
        await ShowAsync(d);
    }

    public static async Task Reply(XamlRoot? root, PRDetailViewModel model, ReviewThread thread)
    {
        if (root is null) return;
        var panel = Ui.V(10, Ui.Text(thread.Location, 12, "muted", mono: true));
        if (thread.Comments.Items.LastOrDefault() is { } last)
        {
            var quote = Ui.H(8, Ui.Avatar(last.Author?.AvatarUrl, 18), Ui.Text(last.Body, 13, "text", wrap: true, maxLines: 6));
            ((TextBlock)quote.Children[1]).MaxWidth = 520;
            panel.Children.Add(Ui.Card(quote, 6, "sunken", 8));
        }
        var text = Editor(L.T("Reply_Placeholder", "Reply (Markdown)"));
        panel.Children.Add(text);
        var canResolve = thread.ViewerCanResolve && !thread.IsResolved;
        var d = Create(root, L.T("Reply_Title", "Reply to thread"), panel, L.T("Reply_Reply", "Reply"), canResolve ? L.T("Reply_AndResolve", "Reply and resolve") : null);
        void Validate()
        {
            var ok = text.Text.Trim().Length > 0;
            d.IsPrimaryButtonEnabled = ok;
            d.IsSecondaryButtonEnabled = ok;
        }
        text.TextChanged += (_, _) => Validate();
        Validate();
        d.PrimaryButtonClick += async (_, args) => await RunWhileOpen(d, () => model.Reply(thread.Id, text.Text), args);
        d.SecondaryButtonClick += async (_, args) => await RunWhileOpen(d, async () =>
        {
            if (!await model.Reply(thread.Id, text.Text)) return false;
            await model.SetResolved(thread.Id, true);
            return true;
        }, args);
        await ShowAsync(d);
    }

    public static async Task InlineComment(XamlRoot? root, PRDetailViewModel model, DiffAnchor anchor)
    {
        if (root is null) return;
        var text = Editor(L.F("Inline_Placeholder", "Comment on line {0} (Markdown)", anchor.Line), minHeight: 100);
        var panel = Ui.V(8, Ui.Text($"{anchor.Path}:{anchor.Line} ({anchor.Side.ToLowerInvariant()})", 12, "muted", mono: true), text);
        var d = Create(root, L.T("Inline_Title", "Comment on line"), panel, L.T("Inline_Single", "Add single comment"), L.T("Inline_AddToReview", "Add to review"));
        ToolTipService.SetToolTip(d, L.T("Inline_Help", "“Add to review” saves the comment as pending and submits it with the review"));
        void Validate()
        {
            var ok = text.Text.Trim().Length > 0;
            d.IsPrimaryButtonEnabled = ok;
            d.IsSecondaryButtonEnabled = ok;
        }
        text.TextChanged += (_, _) => Validate();
        Validate();
        d.PrimaryButtonClick += async (_, args) => await RunWhileOpen(d, () => model.AddInline(anchor, text.Text, true), args);
        d.SecondaryButtonClick += async (_, args) => await RunWhileOpen(d, () => model.AddInline(anchor, text.Text, false), args);
        await ShowAsync(d);
    }

    public static async Task JobLog(XamlRoot? root, GitHubApi api, RepoRef repo, CheckContext check)
    {
        if (root is null || check.DatabaseId is not { } id) return;
        var html = new HtmlView { Height = 560 };
        var errorsOnly = new CheckBox { Content = L.T("Log_ErrorsOnly", "Errors only") };
        string? log = null;
        void Render()
        {
            if (log is null) return;
            html.Render(LogHtml(errorsOnly.IsChecked == true ? FilterErrors(log) : log), "conversation", "bottom", "flush");
        }
        errorsOnly.Checked += (_, _) => Render();
        errorsOnly.Unchecked += (_, _) => Render();
        var toolbar = Ui.H(8, Ui.Glyph(Palette.CheckGlyph(check.Outcome), 14, Palette.CheckKey(check.Outcome)), errorsOnly);
        if (check.Link is { } link) toolbar.Children.Add(Ui.Link(L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(link), 13));
        var panel = new Grid { RowSpacing = 8, MinWidth = 900 };
        panel.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        panel.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        panel.Children.Add(toolbar);
        Grid.SetRow(html, 1);
        panel.Children.Add(html);
        var d = Create(root, check.FullTitle, panel, L.T("Log_Copy", "Copy"), close: L.T("Common_Close", "Close"), maxWidth: 1100);
        d.IsPrimaryButtonEnabled = false;
        d.PrimaryButtonClick += (_, args) =>
        {
            args.Cancel = true;
            if (log is not null) WindowInterop.Copy(log);
        };
        html.Render("<div class=\"tl\"><p class=\"empty\">Loading…</p></div>");
        Task.Run(async () =>
        {
            try
            {
                var raw = await api.JobLog(repo, id);
                return Clean(raw);
            }
            catch (Exception e)
            {
                return "\u0000" + e.Message;
            }
        }).ContinueWith(t => html.DispatcherQueue.TryEnqueue(() =>
        {
            var result = t.Result;
            if (result.StartsWith('\u0000'))
            {
                html.Render($"<div class=\"tl\"><p class=\"empty\">{ConversationHtml.Esc(result[1..])}</p></div>");
                return;
            }
            log = result;
            d.IsPrimaryButtonEnabled = true;
            Render();
        }), TaskScheduler.Default).Forget();
        await ShowAsync(d);
    }

    public static string Clean(string raw)
    {
        var lines = raw.Replace("\r\n", "\n").Split('\n');
        for (var i = 0; i < lines.Length; i++)
        {
            var l = lines[i];
            if (l.Length > 0 && l[0] == '﻿') l = l[1..];
            var space = l.IndexOf(' ');
            if (l.Length > 29 && l.Length > 4 && l[4] == '-' && space >= 20 && space <= 34) l = l[(space + 1)..];
            lines[i] = l;
        }
        return string.Join("\n", lines);
    }

    public static string FilterErrors(string s)
    {
        var lines = s.Split('\n');
        var keep = new List<string>();
        for (var i = 0; i < lines.Length; i++)
        {
            var l = lines[i];
            if (!(l.Contains("##[error]") || l.Contains("error", StringComparison.OrdinalIgnoreCase) || l.Contains("FAILED") || l.Contains('✗'))) continue;
            var from = Math.Max(0, i - 3);
            if (keep.Count > 0 && keep[^1] != "…") keep.Add("…");
            for (var j = from; j <= i; j++) keep.Add(lines[j]);
        }
        return keep.Count == 0 ? "No error lines found" : string.Join("\n", keep);
    }

    private static string LogHtml(string log)
    {
        var sb = new StringBuilder("<pre class=\"log\">");
        foreach (var line in log.Split('\n'))
        {
            var cls = line.Contains("##[error]") ? "err" : line.Contains("##[warning]") ? "warn" : line.StartsWith("##[group]") ? "grp" : null;
            if (cls is not null) sb.Append($"<span class=\"{cls}\">{ConversationHtml.Esc(line)}</span>\n");
            else sb.Append(ConversationHtml.Esc(line)).Append('\n');
        }
        sb.Append("</pre>");
        return sb.ToString();
    }

    public static async Task FixWithClaude(XamlRoot? root, PRDetailViewModel model)
    {
        if (root is null || model.Pr is not { } pr) return;
        var app = AppViewModel.Shared;
        var threads = pr.ReviewThreads.Items.Where(t => !t.IsResolved).ToList();
        var general = FixPrompt.GeneralFeedback(pr);
        var found = FixPromptExtractor.Latest(pr);
        var options = new FixOptions();
        var useReviewer = found is not null;
        var selected = threads.Where(t => !t.IsOutdated).Select(t => t.Id).ToHashSet();
        if (selected.Count == 0) selected = threads.Select(t => t.Id).ToHashSet();
        string? custom = null;
        string? folder = null;

        var promptBox = Editor("", minHeight: 220, mono: true);
        promptBox.FontSize = 11.5;
        promptBox.MaxHeight = 260;
        var editedChip = Ui.Chip(L.T("Fix_Edited", "edited"), "amber");
        var reset = new HyperlinkButton { Content = L.T("Fix_Reset", "Reset"), Padding = new Thickness(0), FontSize = 12 };
        var boxes = new List<(CheckBox, string)>();
        var charCount = Ui.Text("", 11, "dim");
        var updating = false;

        string Generated()
        {
            if (useReviewer && found is not null) return FixPromptExtractor.Wrap(found, pr, options);
            var chosen = threads.Where(t => selected.Contains(t.Id)).ToList();
            return FixPrompt.Build(pr, model.Ref.Repo, chosen, general, options);
        }

        void Regenerate()
        {
            updating = true;
            promptBox.Text = custom ?? Generated();
            updating = false;
            editedChip.Visibility = custom is not null ? Visibility.Visible : Visibility.Collapsed;
            reset.Visibility = editedChip.Visibility;
            charCount.Text = promptBox.Text.Length.ToString();
        }

        reset.Click += (_, _) =>
        {
            custom = null;
            Regenerate();
        };

        promptBox.TextChanged += (_, _) =>
        {
            if (updating) return;
            custom = promptBox.Text;
            editedChip.Visibility = Visibility.Visible;
            reset.Visibility = Visibility.Visible;
            charCount.Text = promptBox.Text.Length.ToString();
        };

        var panel = Ui.V(10);
        panel.Children.Add(Ui.H(10, Ui.IconTile("", "coral", 30), Ui.V(1, Ui.Text(L.T("Fix_Button", "Fix with Claude Code"), 15, "text", FontWeights.SemiBold), Ui.H(6, Ui.Text($"#{pr.Number}", 13, "violet", FontWeights.SemiBold), Ui.Text(pr.Title, 13, "muted")))));

        var threadPanel = Ui.V(4);
        var composedBox = Ui.V(4);
        if (found is not null)
        {
            var bar = new SelectorBar();
            var reviewer = new SelectorBarItem { Text = L.F("Fix_PromptBy", "Fix prompt by @{0}", found.Author) + (found.Date is { } fd ? " · " + Fmt.Rel(fd) : "") };
            var composed = new SelectorBarItem { Text = L.F("Fix_FromComments", "Build from comments ({0})", threads.Count) };
            bar.Items.Add(reviewer);
            bar.Items.Add(composed);
            bar.SelectedItem = reviewer;
            bar.SelectionChanged += (s, _) =>
            {
                useReviewer = s.SelectedItem == reviewer;
                composedBox.Visibility = useReviewer ? Visibility.Collapsed : Visibility.Visible;
                custom = null;
                Regenerate();
            };
            panel.Children.Add(bar);
        }
        else
        {
            panel.Children.Add(Ui.H(6, Ui.Glyph("", 12), Ui.Text(L.T("Fix_NoPrompt", "No Fix prompt in this PR — built from unresolved comments"), 13, "muted")));
        }
        composedBox.Children.Add(Ui.Columns((Ui.Text(L.T("Fix_Unresolved", "Unresolved comments"), 12, "muted", FontWeights.SemiBold), Ui.Star),
            (Ui.H(8, Ui.Link(L.T("Fix_All", "All"), () => SetAll(true)), Ui.Link(L.T("Fix_None", "None"), () => SetAll(false))), Ui.Auto)));
        if (threads.Count == 0) threadPanel.Children.Add(Ui.Text(L.T("Fix_NoUnresolved", "No unresolved comments"), 13, "dim"));
        foreach (var th in threads)
        {
            var label = Ui.H(6, Ui.Text(th.Location, 12, "sky", mono: true));
            if (th.IsOutdated) label.Children.Add(Ui.Chip(L.T("Fix_Outdated", "outdated"), "amber"));
            var first = th.Comments.Items.FirstOrDefault();
            label.Children.Add(Ui.Text($"@{first?.Author?.Login ?? "?"}: {first?.Body.Replace("\n", " ")}", 12, "muted"));
            var cb = new CheckBox { Content = label, IsChecked = selected.Contains(th.Id) };
            var id = th.Id;
            cb.Checked += (_, _) => { selected.Add(id); custom = null; Regenerate(); };
            cb.Unchecked += (_, _) => { selected.Remove(id); custom = null; Regenerate(); };
            boxes.Add((cb, id));
            threadPanel.Children.Add(cb);
        }
        if (general.Count > 0)
        {
            var g = new CheckBox { Content = L.F("Fix_General", "General review feedback ({0})", general.Count), IsChecked = options.IncludeGeneral };
            g.Checked += (_, _) => { options.IncludeGeneral = true; custom = null; Regenerate(); };
            g.Unchecked += (_, _) => { options.IncludeGeneral = false; custom = null; Regenerate(); };
            threadPanel.Children.Add(g);
        }
        void SetAll(bool on)
        {
            foreach (var (cb, _) in boxes) cb.IsChecked = on;
        }
        composedBox.Children.Add(new ScrollViewer { Content = threadPanel, MaxHeight = 130 });
        composedBox.Visibility = useReviewer ? Visibility.Collapsed : Visibility.Visible;
        panel.Children.Add(composedBox);

        var commit = new CheckBox { Content = L.T("Fix_Commit", "Commit"), IsChecked = options.Commit };
        var push = new CheckBox { Content = L.T("Fix_Push", "Push"), IsChecked = options.Push };
        var resolve = new CheckBox { Content = L.T("Fix_ReplyResolve", "Reply & resolve threads"), IsChecked = options.ReplyAndResolve, IsEnabled = options.Push };
        commit.Checked += (_, _) => { options.Commit = true; custom = null; Regenerate(); };
        commit.Unchecked += (_, _) => { options.Commit = false; custom = null; Regenerate(); };
        push.Checked += (_, _) => { options.Push = true; resolve.IsEnabled = true; custom = null; Regenerate(); };
        push.Unchecked += (_, _) => { options.Push = false; resolve.IsChecked = false; resolve.IsEnabled = false; options.ReplyAndResolve = false; custom = null; Regenerate(); };
        resolve.Checked += (_, _) => { options.ReplyAndResolve = true; custom = null; Regenerate(); };
        resolve.Unchecked += (_, _) => { options.ReplyAndResolve = false; custom = null; Regenerate(); };
        panel.Children.Add(Ui.H(16, commit, push, resolve));

        panel.Children.Add(Ui.Columns((Ui.H(6, Ui.Text(L.T("Fix_Prompt", "Prompt"), 12, "muted", FontWeights.SemiBold), editedChip, reset), Ui.Star), (charCount, Ui.Auto)));
        panel.Children.Add(promptBox);

        var folderText = Ui.Text(L.F("Fix_Looking", "Looking for a local clone of {0}…", model.Ref.Repo.Full), 13, "muted", mono: true);
        var folderWarn = Ui.Text("", 12, "amber");
        var choose = Ui.Button(L.T("Fix_ChooseFolder", "Choose folder…"), () => { });
        panel.Children.Add(Ui.Card(Ui.Columns((Ui.Glyph("", 14, "teal"), Ui.Auto), (Ui.V(2, folderText, folderWarn), Ui.Star), (choose, Ui.Auto)), 8, "raised", 8));

        var targets = new ComboBox { Header = L.T("Fix_OpenIn", "Open in"), MinWidth = 200 };
        foreach (var t in Enum.GetValues<ClaudeTarget>().Where(ClaudeLauncher.TargetAvailable)) targets.Items.Add(new ComboBoxItem { Content = t.Title(), Tag = t });
        var savedTarget = Settings.Shared.GetString(Keys.ClaudeTarget) ?? "App";
        targets.SelectedItem = targets.Items.OfType<ComboBoxItem>().FirstOrDefault(i => i.Tag?.ToString() == savedTarget) ?? targets.Items.OfType<ComboBoxItem>().FirstOrDefault();
        var modes = new ComboBox { Header = L.T("Fix_Mode", "Mode"), MinWidth = 200 };
        foreach (var m in Enum.GetValues<ClaudePermissionMode>()) modes.Items.Add(new ComboBoxItem { Content = m.Title(), Tag = m });
        var savedMode = Settings.Shared.GetString(Keys.ClaudePermissionMode) ?? "Standard";
        modes.SelectedItem = modes.Items.OfType<ComboBoxItem>().FirstOrDefault(i => i.Tag?.ToString() == savedMode) ?? modes.Items[0];
        ClaudeTarget Target() => (targets.SelectedItem as ComboBoxItem)?.Tag is ClaudeTarget t ? t : ClaudeTarget.Copy;
        void UpdateModes() => modes.IsEnabled = Target() == ClaudeTarget.Terminal;
        targets.SelectionChanged += (_, _) =>
        {
            Settings.Shared.Set(Keys.ClaudeTarget, Target().ToString());
            UpdateModes();
        };
        modes.SelectionChanged += (_, _) => Settings.Shared.Set(Keys.ClaudePermissionMode, (modes.SelectedItem as ComboBoxItem)?.Tag?.ToString());
        UpdateModes();
        var error = Ui.Text("", 12, "red", wrap: true);
        panel.Children.Add(Ui.H(12, targets, modes));
        panel.Children.Add(error);

        Regenerate();
        var d = Create(root, "", new ScrollViewer { Content = panel, MaxHeight = 640 }, L.T("Fix_Launch", "Launch"), maxWidth: 820);
        d.Title = null;

        void SetFolder(string? f)
        {
            folder = f;
            if (f is null)
            {
                folderText.Text = L.T("Fix_NoClone", "No local clone found");
                folderWarn.Text = L.F("Fix_CloneHint", "Clone it with: gh repo clone {0}", model.Ref.Repo.Full);
            }
            else
            {
                folderText.Text = Fmt.HomeRelative(f);
                folderWarn.Text = LocalRepoStore.RemoteMatches(f, model.Ref.Repo) ? "" : L.F("Fix_RemoteMismatch", "remote doesn't match {0}", model.Ref.Repo.Full);
            }
            Validate();
        }

        void Validate()
        {
            var t = Target();
            d.PrimaryButtonText = t == ClaudeTarget.Copy ? L.T("Fix_Copy", "Copy") : L.T("Fix_Launch", "Launch");
            d.IsPrimaryButtonEnabled = promptBox.Text.Trim().Length > 0 && (t == ClaudeTarget.Copy || folder is not null);
        }

        targets.SelectionChanged += (_, _) => Validate();
        promptBox.TextChanged += (_, _) => Validate();
        choose.Click += async (_, _) =>
        {
            if (App.MainWindow is null) return;
            d.Hide();
            var picked = await WindowInterop.PickFolder(App.MainWindow);
            if (picked is not null)
            {
                LocalRepoStore.Save(model.Ref.Repo, picked);
                SetFolder(picked);
            }
            open = false;
            await ShowAsync(d);
        };
        d.PrimaryButtonClick += (_, args) =>
        {
            var prompt = promptBox.Text;
            var t = Target();
            var name = $"Fix PR #{pr.Number}: {pr.Title}";
            if (t == ClaudeTarget.App && folder is not null)
            {
                app.Claude.NewSession(folder, prompt, name);
                app.Section = Section.Claude;
                return;
            }
            try
            {
                var mode = (modes.SelectedItem as ComboBoxItem)?.Tag is ClaudePermissionMode m ? m : ClaudePermissionMode.Standard;
                ClaudeLauncher.Launch(prompt, folder ?? "", t, mode, name, null, ClaudeProfiles.Shared.ActiveOverride, WindowInterop.Copy);
            }
            catch (Exception e)
            {
                error.Text = e.Message;
                args.Cancel = true;
            }
        };
        DetectFolder().Forget();
        async Task DetectFolder()
        {
            var f = await LocalRepoStore.Detect(model.Ref.Repo);
            SetFolder(f);
        }
        await ShowAsync(d);
    }
}
