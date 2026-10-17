using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using PullYard.Controls;
using PullYard.Core.Auth;
using PullYard.Core.Util;
using PullYard.Services;
using PullYard.ViewModels;

namespace PullYard.Views;

public static class AccountDialogs
{
    public static MenuFlyout Menu(XamlRoot? root)
    {
        var app = AppViewModel.Shared;
        var menu = new MenuFlyout();
        menu.Items.Add(new MenuFlyoutItem { Text = L.T("Accounts_Title", "Accounts"), IsEnabled = false });
        foreach (var a in app.Accounts.All)
        {
            var current = a.Id == app.GitAccount?.Id;
            var target = a;
            menu.Items.Add(Ui.MenuItem($"{a.Provider.Title()} — {a.Subtitle}", () =>
            {
                if (!current) Switch(root, target).Forget();
            }, current ? "" : a.Provider.Glyph()));
        }
        menu.Items.Add(Ui.MenuItem(L.T("Accounts_Add", "Add Account…"), () => Add(root).Forget(), ""));
        menu.Items.Add(Ui.Separator());
        menu.Items.Add(Ui.MenuItem(L.T("Accounts_SignOut", "Sign Out…"), () => ConfirmSignOut(root).Forget(), ""));
        return menu;
    }

    public static async Task Switch(XamlRoot? root, GitAccount target)
    {
        var app = AppViewModel.Shared;
        if (app.Claude.RunningCount > 0 && !await Dialogs.Confirm(root, L.F("Accounts_SwitchTitle", "Switch to {0}?", target.Subtitle), L.T("Accounts_SwitchButton", "Switch"), false,
                L.T("Accounts_SwitchBody", "Running Claude sessions stop. Their history stays, and this account's Claude Code sign-in is kept for when you switch back."))) return;
        try
        {
            await app.SwitchAccount(target.Id);
        }
        catch (Exception e)
        {
            await Dialogs.Alert(root ?? App.MainWindow?.Content?.XamlRoot, L.T("Accounts_SwitchFailed", "Couldn't switch account"), e.Message);
        }
    }

    public static async Task<bool> ConfirmSignOut(XamlRoot? root)
    {
        var app = AppViewModel.Shared;
        var name = app.GitAccount?.Subtitle ?? (app.User is { } u ? "@" + u.Login : L.T("Accounts_ThisAccount", "this account"));
        var message = L.F("Accounts_SignOutBody", "The account is removed from {0}, and the Claude Code accounts linked to it are signed out. Other accounts and their Claude Code sign-ins are not affected.", Core.AppInfo.DisplayName);
        if (!await Dialogs.Confirm(root, L.F("Accounts_SignOutTitle", "Sign out of {0}?", name), L.T("Accounts_SignOutButton", "Sign Out"), message: message)) return false;
        await app.SignOut();
        return true;
    }

    public static async Task Add(XamlRoot? root)
    {
        if (root is null) return;
        ContentDialog? dialog = null;
        var login = new LoginView(embedded: true, onDone: () => dialog?.Hide()) { MaxHeight = 560 };
        dialog = Dialogs.Create(root, L.T("Accounts_AddTitle", "Add account"), login, maxWidth: 600);
        await Dialogs.ShowAsync(dialog);
    }
}

public sealed class AccountFooter : Grid
{
    public AccountFooter(string name, string subtitle, string? avatarUrl)
    {
        var app = AppViewModel.Shared;
        var switcher = Ui.IconButton("", L.T("Accounts_Switch", "Switch account"), () => { });
        switcher.Click += (_, _) => AccountDialogs.Menu(XamlRoot).ShowAt(switcher);
        var settings = Ui.IconButton("", L.T("Common_Settings", "Settings"), app.RequestSettings);
        var info = Ui.V(0, Ui.Text(name, 13, "text", FontWeights.Medium, maxLines: 1), Ui.Caption(subtitle));
        var grid = Ui.Columns((Ui.Avatar(avatarUrl, 26), Ui.Auto), (info, Ui.Star), (switcher, Ui.Auto), (settings, Ui.Auto));
        grid.ColumnSpacing = 8;
        grid.Padding = new Thickness(12, 8, 6, 10);
        Children.Add(grid);
    }
}
