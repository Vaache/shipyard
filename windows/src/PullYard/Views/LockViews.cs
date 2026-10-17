using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using PullYard.Controls;
using PullYard.Core;
using PullYard.Core.Auth;
using PullYard.Core.Util;
using PullYard.Services;
using PullYard.ViewModels;

namespace PullYard.Views;

public enum UnlockOutcome { Cancelled, Unlocked, NeedsSignIn }

public sealed class UnlockView : Grid
{
    private readonly AppViewModel app = AppViewModel.Shared;
    private readonly GitAccount account;
    private readonly bool sheet;
    private readonly Action<UnlockOutcome> finished;
    private readonly PasswordBox pinBox = new() { PlaceholderText = L.T("Lock_Pin", "PIN"), Width = 160, MaxLength = AccountLock.MaxLength, FontSize = 18 };
    private readonly Button unlock;
    private readonly Button hello;
    private readonly TextBlock message = Ui.Text("", 13, "amber", wrap: true);
    private readonly TextBlock countdown = Ui.Text("", 13, "muted");
    private readonly StackPanel entry;
    private readonly TextBlock blockedText;
    private readonly StackPanel resetPanel = Ui.V(8);
    private readonly ProgressRing busy = Ui.Spinner(16);
    private readonly DispatcherTimer timer = new() { Interval = TimeSpan.FromSeconds(1) };
    private DateTimeOffset? waitUntil;
    private bool blocked;
    private bool filtering;

    public UnlockView(GitAccount account, string title, bool sheet, Action<UnlockOutcome> finished)
    {
        this.account = account;
        this.sheet = sheet;
        this.finished = finished;
        busy.Visibility = Visibility.Collapsed;
        message.TextAlignment = TextAlignment.Center;
        message.MaxWidth = 360;
        unlock = Ui.Button(L.T("Lock_Unlock", "Unlock"), Submit, accent: true);
        hello = Ui.Button(L.T("Lock_UseHello", "Use Windows Hello"), () => TryHello().Forget(), glyph: "");
        hello.HorizontalAlignment = HorizontalAlignment.Center;
        pinBox.PasswordChanged += (_, _) =>
        {
            if (filtering) return;
            var digits = AccountLock.Digits(pinBox.Password);
            if (digits != pinBox.Password)
            {
                filtering = true;
                pinBox.Password = digits;
                filtering = false;
            }
            UpdateButtons();
        };
        pinBox.KeyDown += (_, e) =>
        {
            if (e.Key != Windows.System.VirtualKey.Enter) return;
            e.Handled = true;
            Submit();
        };
        entry = Ui.V(10, Center(Ui.H(8, pinBox, unlock)), hello);
        blockedText = Ui.Text(L.T("Lock_Blocked", "Too many wrong PINs. Reset the PIN to continue. This also signs out the Claude Code accounts linked to this account."), 13, "red", wrap: true);
        blockedText.TextAlignment = TextAlignment.Center;
        blockedText.MaxWidth = 360;

        var avatar = Center(Ui.Avatar(account.AvatarUrl, 56));
        var name = Center(Ui.Text(account.Name ?? (account.Login.Length > 0 ? account.Login : account.Provider.Title()), 18, "text", FontWeights.SemiBold));
        var subtitle = Center(Ui.Text($"{account.Provider.Title()} · {account.Subtitle}", 13, "muted"));
        var heading = Center(Ui.Text(title, 15, "text", FontWeights.SemiBold));
        heading.Margin = new Thickness(0, 4, 0, 0);

        var forgot = Ui.Link(L.T("Lock_ForgotPin", "Forgot PIN?"), ShowReset, 13);
        var links = Ui.H(16, forgot);
        if (!sheet) links.Children.Add(Ui.Link(L.T("Lock_UseAnotherAccount", "Use another account"), app.UseAnotherAccount, 13));
        links.HorizontalAlignment = HorizontalAlignment.Center;
        links.Margin = new Thickness(0, 6, 0, 0);
        resetPanel.Visibility = Visibility.Collapsed;
        resetPanel.MaxWidth = 380;
        busy.HorizontalAlignment = HorizontalAlignment.Center;

        var stack = Ui.V(14, avatar, Ui.V(2, name, subtitle), heading, entry, blockedText, message, Center(countdown), links, resetPanel, busy);
        stack.Padding = new Thickness(32);
        stack.HorizontalAlignment = HorizontalAlignment.Center;
        stack.VerticalAlignment = sheet ? VerticalAlignment.Top : VerticalAlignment.Center;
        stack.MaxWidth = 440;
        Children.Add(sheet ? stack : new ScrollViewer { Content = stack, VerticalScrollBarVisibility = ScrollBarVisibility.Auto });

        timer.Tick += (_, _) => UpdateCountdown();
        Loaded += (_, _) =>
        {
            RefreshState();
            timer.Start();
            pinBox.Focus(FocusState.Programmatic);
            if (HelloUsable) TryHello().Forget();
        };
        Unloaded += (_, _) => timer.Stop();
    }

    private static T Center<T>(T element) where T : FrameworkElement
    {
        element.HorizontalAlignment = HorizontalAlignment.Center;
        return element;
    }

    private bool HelloUsable => !blocked && WindowsHello.Available && app.Lock.HelloEnabled(account.Id);

    private bool Waiting => waitUntil is { } until && until > DateTimeOffset.Now;

    private void RefreshState()
    {
        var state = app.Lock.State(account.Id);
        blocked = state?.Result == PinResult.Blocked;
        waitUntil = state?.Result == PinResult.Wait ? state.Until : null;
        UpdateButtons();
        UpdateCountdown();
    }

    private void UpdateButtons()
    {
        entry.Visibility = blocked ? Visibility.Collapsed : Visibility.Visible;
        blockedText.Visibility = blocked ? Visibility.Visible : Visibility.Collapsed;
        hello.Visibility = HelloUsable ? Visibility.Visible : Visibility.Collapsed;
        pinBox.IsEnabled = !Waiting;
        unlock.IsEnabled = !Waiting && AccountLock.IsValidPin(pinBox.Password);
    }

    private void UpdateCountdown()
    {
        if (waitUntil is { } until && until > DateTimeOffset.Now)
        {
            countdown.Text = L.F("Lock_TryAgainIn", "Try again in {0} s", (int)Math.Ceiling((until - DateTimeOffset.Now).TotalSeconds));
            countdown.Visibility = Visibility.Visible;
        }
        else
        {
            if (waitUntil is not null)
            {
                waitUntil = null;
                UpdateButtons();
            }
            countdown.Visibility = Visibility.Collapsed;
        }
    }

    private void Say(string? text)
    {
        message.Text = text ?? "";
        message.Visibility = text is null ? Visibility.Collapsed : Visibility.Visible;
    }

    private void Submit()
    {
        var pin = pinBox.Password;
        if (!AccountLock.IsValidPin(pin) || Waiting || blocked) return;
        var result = app.Lock.Verify(pin, account.Id);
        pinBox.Password = "";
        switch (result.Result)
        {
            case PinResult.Ok:
                finished(UnlockOutcome.Unlocked);
                return;
            case PinResult.Wrong:
                Say(result.AttemptsLeft == 1
                    ? L.T("Lock_WrongOneLeft", "Wrong PIN. 1 attempt left before a pause.")
                    : L.F("Lock_WrongLeft", "Wrong PIN. {0} attempts left before a pause.", result.AttemptsLeft));
                break;
            case PinResult.Wait:
                waitUntil = result.Until;
                Say(L.T("Lock_Wrong", "Wrong PIN."));
                break;
            case PinResult.Blocked:
                blocked = true;
                Say(null);
                ShowReset();
                break;
        }
        UpdateButtons();
        UpdateCountdown();
        pinBox.Focus(FocusState.Programmatic);
    }

    private async Task TryHello()
    {
        if (!HelloUsable) return;
        if (!await WindowsHello.Verify(L.F("Lock_HelloReason", "Unlock {0} in {1}", account.Subtitle, AppInfo.DisplayName))) return;
        if (app.Lock.HelloSucceeded(account.Id)) finished(UnlockOutcome.Unlocked);
    }

    private void ShowReset()
    {
        resetPanel.Children.Clear();
        var text = WindowsHello.Available
            ? L.T("Lock_ResetWithHello", "Verify with Windows Hello to remove the PIN. The Claude Code accounts linked to this account are signed out.")
            : L.T("Lock_ResetWithoutHello", "Removing the PIN also signs out of this account and of the Claude Code accounts linked to it. You'll sign in again with a token or the CLI.");
        var body = Ui.Text(text, 12.5, "muted", wrap: true);
        body.TextAlignment = TextAlignment.Center;
        var reset = Ui.Button(WindowsHello.Available ? L.T("Lock_ResetHelloButton", "Reset PIN with Windows Hello") : L.T("Lock_ResetButton", "Remove PIN and Sign Out"), () => Reset().Forget(), accent: true);
        reset.HorizontalAlignment = HorizontalAlignment.Center;
        resetPanel.Children.Add(body);
        resetPanel.Children.Add(reset);
        resetPanel.Visibility = Visibility.Visible;
    }

    private async Task Reset()
    {
        busy.Visibility = Visibility.Visible;
        try
        {
            switch (await app.ResetLock(account))
            {
                case LockReset.Unlocked:
                    finished(UnlockOutcome.Unlocked);
                    break;
                case LockReset.NeedsSignIn:
                    finished(UnlockOutcome.NeedsSignIn);
                    break;
            }
        }
        finally
        {
            busy.Visibility = Visibility.Collapsed;
        }
    }
}

public sealed class LockScreen : Grid
{
    public LockScreen(GitAccount account)
    {
        var app = AppViewModel.Shared;
        Children.Add(new UnlockView(account, L.T("Lock_EnterPin", "Enter PIN"), false, outcome =>
        {
            if (outcome == UnlockOutcome.Unlocked) app.Unlocked(account).Forget();
        }));
    }
}

public static class LockDialogs
{
    public static async Task<UnlockOutcome> Unlock(XamlRoot? root, GitAccount account, string title)
    {
        if (root is null) return UnlockOutcome.Cancelled;
        ContentDialog? dialog = null;
        var outcome = UnlockOutcome.Cancelled;
        var view = new UnlockView(account, title, true, result =>
        {
            outcome = result;
            dialog?.Hide();
        });
        dialog = Dialogs.Create(root, "", view, maxWidth: 480);
        await Dialogs.ShowAsync(dialog);
        return outcome;
    }

    public static async Task<bool> SetPin(XamlRoot? root, GitAccount account)
    {
        if (root is null) return false;
        var pin = new PasswordBox { PlaceholderText = L.T("Lock_NewPin", "New PIN"), MaxLength = AccountLock.MaxLength };
        var confirm = new PasswordBox { PlaceholderText = L.T("Lock_RepeatPin", "Repeat PIN"), MaxLength = AccountLock.MaxLength };
        var error = Ui.Text("", 12, "red", wrap: true);
        var panel = Ui.V(10,
            Ui.Text(L.F("Lock_SetPinBody", "{0} asks for the PIN when it starts with this account and when someone switches to it. Use 4 to 6 digits.", AppInfo.DisplayName), 13, "muted", wrap: true),
            pin, confirm, error);
        var d = Dialogs.Create(root, L.F("Lock_SetPinTitle", "Protect {0} with a PIN", account.Subtitle), panel, L.T("Lock_SetPinButton", "Set PIN"));
        var saved = false;
        d.PrimaryButtonClick += (_, args) =>
        {
            if (!AccountLock.IsValidPin(pin.Password))
            {
                error.Text = L.T("Lock_PinFormat", "Use 4 to 6 digits.");
                args.Cancel = true;
                return;
            }
            if (pin.Password != confirm.Password)
            {
                error.Text = L.T("Lock_PinMismatch", "The PINs don't match.");
                args.Cancel = true;
                return;
            }
            AppViewModel.Shared.Lock.SetPin(pin.Password, account.Id);
            saved = true;
        };
        await Dialogs.ShowAsync(d);
        return saved;
    }
}
