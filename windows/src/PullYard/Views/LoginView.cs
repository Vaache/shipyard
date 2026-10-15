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
    private readonly AppViewModel app = AppViewModel.Shared;
    private readonly ProgressRing busyRing = Ui.Spinner(18);
    private readonly InfoBar errorBar = new() { Severity = InfoBarSeverity.Error, IsClosable = true, IsOpen = false };
    private readonly PasswordBox tokenBox = new() { PlaceholderText = "ghp_… or github_pat_…" };
    private readonly TextBox clientIdBox = new() { PlaceholderText = L.T("Login_ClientId", "Client ID") };
    private readonly StackPanel devicePanel = Ui.V(8);
    private CancellationTokenSource? deviceCts;

    public LoginView()
    {
        busyRing.Visibility = Visibility.Collapsed;
        busyRing.HorizontalAlignment = HorizontalAlignment.Center;
        var icon = new Image { Source = new BitmapImage(new Uri("ms-appx:///Assets/AppIcon-256.png")), Width = 104, Height = 104, HorizontalAlignment = HorizontalAlignment.Center };
        var title = Ui.Text(AppInfo.DisplayName, 30, "text", FontWeights.Bold);
        title.HorizontalAlignment = HorizontalAlignment.Center;
        var subtitle = Ui.Text(L.T("Login_Subtitle", "Fast PR browsing without a browser"), 14, "muted");
        subtitle.HorizontalAlignment = HorizontalAlignment.Center;

        var stack = Ui.V(18, Ui.V(6, icon, title, subtitle));
        stack.MaxWidth = 520;
        stack.Padding = new Thickness(40);
        stack.HorizontalAlignment = HorizontalAlignment.Center;

        if (GhCli.Binary is not null)
        {
            var gh = Ui.Button(L.T("Login_SignInWithGh", "Sign in with gh"), () => RunLogin(async () => await app.Login(await GhCli.Token())), accent: true, glyph: "");
            gh.HorizontalAlignment = HorizontalAlignment.Stretch;
            stack.Children.Add(Card(L.T("Login_GhTitle", "GitHub CLI"), "", "sky",
                Ui.Text(L.T("Login_GhBody", "Use the token you're signed in with in gh."), 13, "muted", wrap: true), gh));
        }

        tokenBox.KeyDown += (_, e) =>
        {
            if (e.Key == Windows.System.VirtualKey.Enter) LoginWithToken();
        };
        var signIn = Ui.Button(L.T("Login_SignIn", "Sign in"), LoginWithToken, accent: true);
        var tokenRow = Ui.Columns((tokenBox, Ui.Star), (signIn, Ui.Auto));
        var create = Ui.Link(L.T("Login_CreateToken", "Create a token on github.com"), () =>
            LinkBrowser.Open($"https://github.com/settings/tokens/new?scopes=repo,read:org,read:project,project,notifications,workflow&description={Uri.EscapeDataString(AppInfo.DisplayName)}"));
        stack.Children.Add(Card(L.T("Login_PatTitle", "Personal Access Token"), "", "amber",
            Ui.Text(L.T("Login_PatBody", "Required scopes: repo, read:org, read:project, project, notifications, workflow."), 13, "muted", wrap: true), tokenRow, create));

        clientIdBox.Text = Settings.Shared.GetString(Keys.OAuthClientId) ?? "";
        clientIdBox.TextChanged += (_, _) => Settings.Shared.Set(Keys.OAuthClientId, clientIdBox.Text);
        RenderDevice(null);
        var expander = new Expander
        {
            Header = Ui.H(8, Ui.IconTile("", "indigo", 22), Ui.Text(L.T("Login_DeviceTitle", "Sign in via browser (OAuth Device Flow)"), 14, "text", FontWeights.SemiBold)),
            Content = Ui.V(8, Ui.Text(L.T("Login_DeviceBody", "Requires the Client ID of your own OAuth App (Settings → Developer settings → OAuth Apps, enable “Enable Device Flow”)."), 13, "muted", wrap: true), clientIdBox, devicePanel),
            HorizontalAlignment = HorizontalAlignment.Stretch,
            HorizontalContentAlignment = HorizontalAlignment.Stretch
        };
        stack.Children.Add(expander);
        stack.Children.Add(busyRing);
        stack.Children.Add(errorBar);

        Children.Add(new ScrollViewer { Content = stack, VerticalScrollBarVisibility = ScrollBarVisibility.Auto });
        Unloaded += (_, _) => deviceCts?.Cancel();
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
            var get = Ui.Button(L.T("Login_GetCode", "Get code"), StartDevice);
            devicePanel.Children.Add(get);
            return;
        }
        var codeText = Ui.Text(code.UserCode, 24, "text", FontWeights.Bold, mono: true, select: true);
        var copy = Ui.Button(L.T("Login_CopyAndOpen", "Copy and open"), () =>
        {
            WindowInterop.Copy(code.UserCode);
            LinkBrowser.Open(code.VerificationUri);
        }, accent: true);
        devicePanel.Children.Add(Ui.H(12, Ui.Card(codeText, 8, "sunken", 6), copy, Ui.Spinner(16)));
        devicePanel.Children.Add(Ui.Text(L.T("Login_DeviceHint", "Enter the code at github.com/login/device — you'll be signed in automatically."), 12, "muted", wrap: true));
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

    private void LoginWithToken()
    {
        var t = tokenBox.Password.Trim();
        if (t.Length == 0) return;
        RunLogin(() => app.Login(t));
    }

    private async void StartDevice()
    {
        var id = clientIdBox.Text.Trim();
        if (id.Length == 0) return;
        ShowError(null);
        deviceCts?.Cancel();
        var cts = new CancellationTokenSource();
        deviceCts = cts;
        try
        {
            var code = await DeviceFlow.Start(id, cts.Token);
            RenderDevice(code);
            WindowInterop.Copy(code.UserCode);
            LinkBrowser.Open(code.VerificationUri);
            var token = await DeviceFlow.Poll(id, code, cts.Token);
            await app.Login(token);
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception e)
        {
            ShowError(e.Message);
        }
        RenderDevice(null);
    }
}
