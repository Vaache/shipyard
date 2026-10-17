using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using PullYard.Core;
using PullYard.Core.Util;
using PullYard.Services;
using PullYard.ViewModels;
using PullYard.Views;
using Windows.System;

namespace PullYard;

public sealed partial class MainWindow : Window
{
    private readonly AppViewModel model = AppViewModel.Shared;
    private bool splashHold = true;
    private Phase? shownPhase;

    public MainWindow()
    {
        InitializeComponent();
        Title = AppInfo.DisplayName;
        TitleText.Text = AppInfo.DisplayName;
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(TitleBarGrid);
        try
        {
            SystemBackdrop = new MicaBackdrop();
        }
        catch
        {
        }
        AppWindow.SetIcon(Path.Combine(AppContext.BaseDirectory, "Assets", "AppIcon.ico"));
        AppWindow.Resize(new Windows.Graphics.SizeInt32(1380, 860));
        if (AppWindow.Presenter is OverlappedPresenter presenter)
        {
            presenter.PreferredMinimumWidth = 960;
            presenter.PreferredMinimumHeight = 600;
        }
        AppWindow.Closing += OnClosing;
        Activated += OnActivated;
        model.IsShown = () => AppWindow.IsVisible && AppWindow.Presenter is not OverlappedPresenter { State: OverlappedPresenterState.Minimized };
        ThemeService.Attach(RootGrid);
        ThemeService.Changed += ApplyBackground;
        ApplyBackground();
        AddAccelerators();
        model.PropertyChanged += (_, e) =>
        {
            if (e.PropertyName == nameof(AppViewModel.Phase)) UpdateContent();
        };
        model.ActivateRequested += () => App.ShowMainWindow();
        model.Updater.PresentRequested += () => UpdateDialog.Show(RootGrid.XamlRoot).Forget();
        model.SettingsRequested += () => SettingsDialog.Show(RootGrid.XamlRoot).Forget();
        model.OpenLinkRequested += () => OpenLinkDialog.Show(RootGrid.XamlRoot).Forget();
        Host.Content = new SplashView();
        HoldSplash().Forget();
    }

    private async Task HoldSplash()
    {
        await Task.Delay(750);
        splashHold = false;
        UpdateContent();
        await model.Updater.CheckOnLaunch();
    }

    private void UpdateContent()
    {
        if (splashHold || model.Phase == Phase.Checking)
        {
            if (!splashHold) shownPhase = null;
            if (Host.Content is not SplashView) Host.Content = new SplashView();
            return;
        }
        if (shownPhase == model.Phase) return;
        shownPhase = model.Phase;
        UIElement next = model.Phase == Phase.LoggedOut ? new LoginView() : new ShellView();
        next.Transitions = new TransitionCollection { new EntranceThemeTransition { FromVerticalOffset = 12 } };
        Host.Content = next;
        SubtitleText.Text = "";
    }

    public void SetSubtitle(string text) => SubtitleText.Text = text.Length > 0 ? "— " + text : "";

    private void ApplyBackground()
    {
        RootGrid.Background = ThemeService.IsBrand
            ? new SolidColorBrush(Palette.Hex(RootGrid.ActualTheme == ElementTheme.Dark ? 0x0C1220u : 0xF7F5F0u))
            : new SolidColorBrush(Microsoft.UI.Colors.Transparent);
        var titleBar = AppWindow.TitleBar;
        var dark = ThemeService.IsDark;
        titleBar.ButtonBackgroundColor = Microsoft.UI.Colors.Transparent;
        titleBar.ButtonInactiveBackgroundColor = Microsoft.UI.Colors.Transparent;
        titleBar.ButtonForegroundColor = dark ? Microsoft.UI.Colors.White : Microsoft.UI.Colors.Black;
        titleBar.ButtonHoverBackgroundColor = dark ? Windows.UI.Color.FromArgb(0x20, 255, 255, 255) : Windows.UI.Color.FromArgb(0x18, 0, 0, 0);
    }

    private void OnActivated(object sender, WindowActivatedEventArgs args)
    {
        var active = args.WindowActivationState != WindowActivationState.Deactivated;
        model.IsActive = active;
        if (!active) return;
        model.DidBecomeActive();
        CheckClipboard().Forget();
    }

    private async Task CheckClipboard()
    {
        if (model.Phase != Phase.LoggedIn) return;
        model.CheckClipboard(await WindowInterop.ReadClipboardText());
    }

    private void OnClosing(AppWindow sender, AppWindowClosingEventArgs args)
    {
        if (App.Quitting || Program.SmokeUi) return;
        if (Settings.Shared.GetBool(Keys.CloseToTray, true))
        {
            args.Cancel = true;
            sender.Hide();
            return;
        }
        args.Cancel = true;
        App.Quit();
    }

    private void AddAccelerators()
    {
        void Add(VirtualKey key, VirtualKeyModifiers mods, Action action)
        {
            var acc = new KeyboardAccelerator { Key = key, Modifiers = mods };
            acc.Invoked += (_, e) =>
            {
                e.Handled = true;
                action();
            };
            RootGrid.KeyboardAccelerators.Add(acc);
        }
        RootGrid.KeyboardAcceleratorPlacementMode = KeyboardAcceleratorPlacementMode.Hidden;
        Add(VirtualKey.R, VirtualKeyModifiers.Control, () => model.Refresh());
        Add(VirtualKey.F5, VirtualKeyModifiers.None, () => model.Refresh());
        Add(VirtualKey.L, VirtualKeyModifiers.Control, () => { if (model.Phase == Phase.LoggedIn) model.RequestOpenLink(); });
        Add((VirtualKey)188, VirtualKeyModifiers.Control, () => model.RequestSettings());
        Add(VirtualKey.Number1, VirtualKeyModifiers.Control, () => model.Section = Section.Prs(PRFilter.ReviewRequested));
        Add(VirtualKey.Number2, VirtualKeyModifiers.Control, () => model.Section = Section.Prs(PRFilter.Mine));
        Add(VirtualKey.Number3, VirtualKeyModifiers.Control, () => model.Section = Section.Notifications);
        Add(VirtualKey.Number4, VirtualKeyModifiers.Control, () => model.Section = Section.Repos);
        Add(VirtualKey.Number5, VirtualKeyModifiers.Control, () => model.Section = Section.Projects);
        Add(VirtualKey.Number6, VirtualKeyModifiers.Control, () => model.Section = Section.Claude);
        Add(VirtualKey.Left, VirtualKeyModifiers.Menu, () => ShellView.Current?.GoBack());
        Add(VirtualKey.Q, VirtualKeyModifiers.Control, App.Quit);
    }
}
