using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media.Imaging;
using PullYard.Controls;
using PullYard.Core;
using PullYard.Core.Auth;
using PullYard.Core.Util;
using PullYard.Services;
using PullYard.ViewModels;

namespace PullYard.Views;

public sealed class LoginView : Grid
{
    private const string ProviderKey = "loginProvider";

    private readonly AppViewModel app = AppViewModel.Shared;
    private readonly bool embedded;
    private readonly Action? onDone;
    private readonly ProgressRing busyRing = Ui.Spinner(18);
    private readonly InfoBar errorBar = new() { Severity = InfoBarSeverity.Error, IsClosable = true, IsOpen = false };
    private readonly StackPanel providerArea = Ui.V(18);
    private readonly TextBox hostBox = new();
    private readonly TextBox emailBox = new() { PlaceholderText = L.T("Login_BitbucketEmail", "Atlassian account email") };
    private readonly PasswordBox tokenBox = new();
    private readonly TextBox clientIdBox = new();
    private readonly StackPanel devicePanel = Ui.V(8);
    private readonly SelectorBar picker = new() { HorizontalAlignment = HorizontalAlignment.Center };
    private GitProvider provider;
    private GitAccount? prefill;
    private CancellationTokenSource? deviceCts;

    public LoginView(bool embedded = false, Action? onDone = null)
    {
        this.embedded = embedded;
        this.onDone = onDone;
        busyRing.Visibility = Visibility.Collapsed;
        busyRing.HorizontalAlignment = HorizontalAlignment.Center;
        provider = Enum.TryParse<GitProvider>(Settings.Shared.GetString(ProviderKey), out var saved) ? saved : GitProvider.GitHub;

        var stack = Ui.V(18);
        stack.MaxWidth = 520;
        stack.Padding = new Thickness(embedded ? 4 : 40);
        stack.HorizontalAlignment = embedded ? HorizontalAlignment.Stretch : HorizontalAlignment.Center;

        if (!embedded)
        {
            var icon = new Image { Source = new BitmapImage(new Uri("ms-appx:///Assets/AppIcon-256.png")), Width = 104, Height = 104, HorizontalAlignment = HorizontalAlignment.Center };
            var title = Ui.Text(AppInfo.DisplayName, 30, "text", FontWeights.Bold);
            title.HorizontalAlignment = HorizontalAlignment.Center;
            var subtitle = Ui.Text(L.T("Login_Subtitle", "Fast PR browsing without a browser"), 14, "muted");
            subtitle.HorizontalAlignment = HorizontalAlignment.Center;
            stack.Children.Add(Ui.V(6, icon, title, subtitle));
            if (SavedAccounts() is { } accounts) stack.Children.Add(accounts);
        }

        foreach (var p in GitProviders.All)
        {
            var item = new SelectorBarItem { Text = p.ShortTitle(), Tag = p };
            Ui.Tip(item, p.Title());
            picker.Items.Add(item);
            if (p == provider) picker.SelectedItem = item;
        }
        picker.SelectionChanged += (s, _) =>
        {
            if (s.SelectedItem?.Tag is not GitProvider p || p == provider) return;
            provider = p;
            Settings.Shared.Set(ProviderKey, p.ToString());
            RenderProvider();
        };
        stack.Children.Add(picker);
        stack.Children.Add(providerArea);

        stack.Children.Add(busyRing);
        stack.Children.Add(errorBar);

        tokenBox.KeyDown += (_, e) =>
        {
            if (e.Key == Windows.System.VirtualKey.Enter) LoginWithToken();
        };
        clientIdBox.TextChanged += (_, _) => Settings.Shared.Set(ClientIdKey, clientIdBox.Text);
        RenderProvider();

        Children.Add(new ScrollViewer { Content = stack, VerticalScrollBarVisibility = ScrollBarVisibility.Auto });
        Unloaded += (_, _) => deviceCts?.Cancel();
    }

    private string ClientIdKey => provider == GitProvider.GitHub ? Keys.OAuthClientId : $"{Keys.OAuthClientId}.{provider}";

    private string Host => provider.EffectiveHost(hostBox.Text);

    private FrameworkElement? SavedAccounts()
    {
        var accounts = app.Accounts.All;
        if (accounts.Count == 0) return null;
        var rows = new List<UIElement>();
        foreach (var a in accounts)
        {
            var id = a.Id;
            var info = Ui.V(0, Ui.Text(a.Name ?? (a.Login.Length > 0 ? a.Login : a.Provider.Title()), 13, "text", FontWeights.Medium), Ui.Caption($"{a.Provider.Title()} · {a.Subtitle}"));
            var target = a;
            var go = app.Accounts.HasCredential(a)
                ? Ui.Button(L.T("Login_Continue", "Continue"), () => RunLogin(() => Continue(target)))
                : Ui.Button(L.T("Login_SignInAgain", "Sign in again"), () => Prefill(target));
            var row = Ui.Columns((Ui.Avatar(a.AvatarUrl, 24), Ui.Auto), (info, Ui.Star), (go, Ui.Auto));
            row.ColumnSpacing = 10;
            rows.Add(row);
        }
        return Card(L.T("Login_YourAccounts", "Your accounts"), "", "green", rows.ToArray());
    }

    private async Task Continue(GitAccount account)
    {
        if (app.Lock.IsProtected(account.Id))
        {
            switch (await LockDialogs.Unlock(XamlRoot, account, L.T("Lock_EnterPin", "Enter PIN")))
            {
                case UnlockOutcome.Cancelled:
                    return;
                case UnlockOutcome.NeedsSignIn:
                    Prefill(account);
                    return;
            }
        }
        await app.SwitchAccount(account.Id);
    }

    private void Prefill(GitAccount account)
    {
        prefill = account;
        if (provider == account.Provider) RenderProvider();
        else picker.SelectedItem = picker.Items.FirstOrDefault(i => i.Tag is GitProvider p && p == account.Provider);
    }

    private void RenderProvider()
    {
        deviceCts?.Cancel();
        ShowError(null);
        providerArea.Children.Clear();
        hostBox.Text = provider.DefaultHost();
        hostBox.PlaceholderText = provider switch
        {
            GitProvider.GitHubEnterprise => "github.example.com",
            GitProvider.GitLab => L.T("Login_GitLabHostPlaceholder", "gitlab.com or gitlab.example.com"),
            GitProvider.Gitea => L.T("Login_GiteaHostPlaceholder", "codeberg.org or git.example.com"),
            _ => provider.DefaultHost()
        };
        tokenBox.Password = "";
        tokenBox.PlaceholderText = provider.TokenPlaceholder();
        clientIdBox.PlaceholderText = provider == GitProvider.GitLab ? L.T("Login_ApplicationId", "Application ID") : L.T("Login_ClientId", "Client ID");
        clientIdBox.Text = Settings.Shared.GetString(ClientIdKey) ?? "";
        if (prefill is { } again && again.Provider == provider)
        {
            if (provider.HostEditable()) hostBox.Text = again.Host;
            emailBox.Text = again.Email ?? "";
            prefill = null;
            DispatcherQueue.TryEnqueue(() => tokenBox.Focus(FocusState.Programmatic));
        }

        if (provider.HostEditable())
            providerArea.Children.Add(Card(L.F("Login_ServerTitle", "{0} server", provider.Title()), "", "teal", hostBox));

        var cli = provider switch
        {
            GitProvider.GitHub or GitProvider.GitHubEnterprise when GhCli.Binary is not null => (Title: L.T("Login_GhTitle", "GitHub CLI"), Body: L.T("Login_GhBody", "Use the token you're signed in with in gh."), Button: L.T("Login_SignInWithGh", "Sign in with gh")),
            GitProvider.GitLab when GLabCli.Binary is not null => (Title: L.T("Login_GlabTitle", "GitLab CLI"), Body: L.T("Login_GlabBody", "Use the token you're signed in with in glab."), Button: L.T("Login_SignInWithGlab", "Sign in with glab")),
            _ => default
        };
        if (cli.Title is not null)
        {
            var button = Ui.Button(cli.Button, SignInWithCli, accent: true, glyph: "");
            button.HorizontalAlignment = HorizontalAlignment.Stretch;
            providerArea.Children.Add(Card(cli.Title, "", "sky", Ui.Text(cli.Body, 13, "muted", wrap: true), button));
        }

        var signIn = Ui.Button(L.T("Login_SignIn", "Sign in"), LoginWithToken, accent: true);
        var tokenRow = Ui.Columns((tokenBox, Ui.Star), (signIn, Ui.Auto));
        tokenRow.ColumnSpacing = 8;
        var tokenContent = new List<UIElement> { Ui.Text(provider.TokenHint(), 13, "muted", wrap: true) };
        if (provider == GitProvider.Bitbucket) tokenContent.Add(emailBox);
        tokenContent.Add(tokenRow);
        tokenContent.Add(Ui.Link(provider == GitProvider.Bitbucket ? L.T("Login_CreateAtlassianToken", "Create an API token at id.atlassian.com") : L.F("Login_CreateTokenOn", "Create a token on {0}", provider == GitProvider.GitHub ? GitProviders.GitHubHost : L.T("Login_YourServer", "your server")), () =>
        {
            if (provider.TokenCreateUrl(Host) is { } url) LinkBrowser.Open(url);
            else ShowError(L.T("Login_EnterServer", "Enter the server address"));
        }));
        providerArea.Children.Add(Card(provider.TokenTitle(), "", "amber", tokenContent.ToArray()));

        if (provider.SupportsDeviceFlow())
        {
            RenderDevice(null);
            providerArea.Children.Add(new Expander
            {
                Header = Ui.H(8, Ui.IconTile("", "indigo", 22), Ui.Text(L.T("Login_DeviceTitle", "Sign in via browser (OAuth Device Flow)"), 14, "text", FontWeights.SemiBold)),
                Content = Ui.V(8, Ui.Text(provider.DeviceHelp(), 13, "muted", wrap: true), clientIdBox, devicePanel),
                HorizontalAlignment = HorizontalAlignment.Stretch,
                HorizontalContentAlignment = HorizontalAlignment.Stretch
            });
        }
    }

    private static Border Card(string title, string glyph, string color, params UIElement[] content)
    {
        var panel = Ui.V(8, Ui.H(8, Ui.IconTile(glyph, color, 22), Ui.Text(title, 14, "text", FontWeights.SemiBold)));
        foreach (var c in content) panel.Children.Add(c);
        return Ui.Card(panel, 12, "raised", 16);
    }

    private void RenderDevice(DeviceCode? code)
    {
        devicePanel.Children.Clear();
        if (code is null)
        {
            devicePanel.Children.Add(Ui.Button(L.T("Login_GetCode", "Get code"), StartDevice));
            return;
        }
        var codeText = Ui.Text(code.UserCode, 24, "text", FontWeights.Bold, mono: true, select: true);
        var copy = Ui.Button(L.T("Login_CopyAndOpen", "Copy and open"), () =>
        {
            WindowInterop.Copy(code.UserCode);
            LinkBrowser.Open(code.VerificationUri);
        }, accent: true);
        devicePanel.Children.Add(Ui.H(12, Ui.Card(codeText, 8, "sunken", 6), copy, Ui.Spinner(16)));
        devicePanel.Children.Add(Ui.Text(L.F("Login_DeviceHintAt", "Enter the code at {0} — you'll be signed in automatically.", code.VerificationUri), 12, "muted", wrap: true));
    }

    private void ShowError(string? message)
    {
        errorBar.Message = message ?? "";
        errorBar.IsOpen = message is not null;
    }

    private async void RunLogin(Func<Task> op)
    {
        busyRing.Visibility = Visibility.Visible;
        ShowError(null);
        try
        {
            await op();
        }
        catch (Exception e)
        {
            ShowError(e.Message);
        }
        finally
        {
            busyRing.Visibility = Visibility.Collapsed;
        }
    }

    private async Task Finish(GitProvider p, string host, GitCredential credential, string? email)
    {
        await app.AddAccount(p, host, credential, email);
        onDone?.Invoke();
    }

    private void SignInWithCli()
    {
        var p = provider;
        var host = Host;
        if (host.Length == 0)
        {
            ShowError(L.T("Login_EnterServer", "Enter the server address"));
            return;
        }
        RunLogin(async () =>
        {
            var token = p == GitProvider.GitLab ? await GLabCli.Token(host) : await GhCli.Token(host);
            await Finish(p, host, GitCredential.Of(token), null);
        });
    }

    private void LoginWithToken()
    {
        var p = provider;
        var host = Host;
        var token = tokenBox.Password.Trim();
        var email = emailBox.Text.Trim();
        if (token.Length == 0) return;
        if (host.Length == 0)
        {
            ShowError(L.T("Login_EnterServer", "Enter the server address"));
            return;
        }
        if (p == GitProvider.Bitbucket && email.Length == 0)
        {
            ShowError(L.T("Login_EnterEmail", "Enter your Atlassian account email"));
            return;
        }
        RunLogin(() => Finish(p, host, GitCredential.Of(token), p == GitProvider.Bitbucket ? email : null));
    }

    private async void StartDevice()
    {
        var id = clientIdBox.Text.Trim();
        var p = provider;
        var host = Host;
        if (id.Length == 0) return;
        if (host.Length == 0)
        {
            ShowError(L.T("Login_EnterServer", "Enter the server address"));
            return;
        }
        var endpoint = p == GitProvider.GitLab ? DeviceFlow.GitLab(host) : DeviceFlow.GitHub(host);
        ShowError(null);
        deviceCts?.Cancel();
        var cts = new CancellationTokenSource();
        deviceCts = cts;
        try
        {
            var code = await DeviceFlow.Start(id, endpoint, cts.Token);
            RenderDevice(code);
            WindowInterop.Copy(code.UserCode);
            LinkBrowser.Open(code.VerificationUri);
            var token = await DeviceFlow.Poll(id, code, endpoint, cts.Token);
            var credential = p == GitProvider.GitLab ? token.Credential(id) : GitCredential.Of(token.AccessToken);
            await Finish(p, host, credential, null);
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception e)
        {
            ShowError(e.Message);
        }
        if (ReferenceEquals(deviceCts, cts)) RenderDevice(null);
    }
}
