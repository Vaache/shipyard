using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using PullYard.Core.Git;
using PullYard.Core.Util;

namespace PullYard.Core.Claude;

public enum ClaudeTabState { Working, Waiting, Active, Sleeping, Stopped }

public static class ClaudeTabStates
{
    public static string Title(this ClaudeTabState s) => s switch
    {
        ClaudeTabState.Working => "Working",
        ClaudeTabState.Waiting => "Needs you",
        ClaudeTabState.Active => "Active",
        ClaudeTabState.Sleeping => "Sleeping",
        _ => "Stopped"
    };

    public static string Help(this ClaudeTabState s) => s switch
    {
        ClaudeTabState.Sleeping => "Process stopped, memory freed. A new message resumes the session.",
        ClaudeTabState.Active => "Process is running and uses memory. Sleeps after idle time.",
        _ => s.Title()
    };

    public static string ColorKey(this ClaudeTabState s) => s switch
    {
        ClaudeTabState.Working => "sky",
        ClaudeTabState.Waiting => "amber",
        ClaudeTabState.Active => "green",
        ClaudeTabState.Sleeping => "indigo",
        _ => "dim"
    };
}

public sealed partial class ClaudeTab : ObservableObject
{
    public Guid Id { get; } = Guid.NewGuid();
    public string Folder { get; }
    public ClaudeTabKind Kind { get; }
    public GitWorkingCopy Git { get; }
    public ChatSession? Chat { get; }
    public string? ConfigDir { get; }

    [ObservableProperty] private string title;
    [ObservableProperty] private string sessionId;
    [ObservableProperty] private bool terminalRunning;
    [ObservableProperty] private bool terminalSleeping;
    [ObservableProperty] private long terminalMemory;
    [ObservableProperty] private int? exitCode;
    [ObservableProperty] private ClaudeTabState state = ClaudeTabState.Stopped;

    public TerminalSession? Terminal { get; set; }
    public DateTimeOffset StartedAt { get; set; } = DateTimeOffset.Now;

    public ClaudeTab(string folder, ClaudeTabKind kind, string title, string sessionId, GitWorkingCopy git)
    {
        Folder = folder;
        Kind = kind;
        this.title = title;
        this.sessionId = sessionId;
        Git = git;
        ConfigDir = ClaudeProfiles.Shared.ActiveOverride;
        if (kind == ClaudeTabKind.Chat)
        {
            Chat = new ChatSession(folder, ConfigDir);
            Chat.PropertyChanged += (_, _) => UpdateState();
        }
    }

    public string ProjectName => System.IO.Path.GetFileName(Folder.TrimEnd('\\', '/')) is { Length: > 0 } n ? n : Folder;

    public bool Running => Kind == ClaudeTabKind.Chat ? Chat?.Running ?? false : TerminalRunning;
    public bool Busy => Chat?.Busy ?? false;
    public bool NeedsAttention => (Chat?.PendingCount ?? 0) > 0;
    public long MemoryBytes => Chat?.MemoryBytes ?? TerminalMemory;

    public int Pid => Chat is not null ? Chat.Pid : TerminalRunning ? Terminal?.ProcessId ?? 0 : 0;

    partial void OnTerminalRunningChanged(bool value) => UpdateState();

    partial void OnTerminalSleepingChanged(bool value) => UpdateState();

    partial void OnTerminalMemoryChanged(long value) => OnPropertyChanged(nameof(MemoryBytes));

    public void UpdateState()
    {
        ClaudeTabState next;
        if (Chat is { } c)
        {
            if (c.PendingCount > 0) next = ClaudeTabState.Waiting;
            else if (c.Busy) next = ClaudeTabState.Working;
            else if (c.Running) next = ClaudeTabState.Active;
            else next = c.Sleeping ? ClaudeTabState.Sleeping : ClaudeTabState.Stopped;
        }
        else if (TerminalRunning)
        {
            next = Terminal is { } t && DateTimeOffset.Now - t.LastOutput < TimeSpan.FromSeconds(3) ? ClaudeTabState.Working : ClaudeTabState.Active;
        }
        else
        {
            next = TerminalSleeping ? ClaudeTabState.Sleeping : ClaudeTabState.Stopped;
        }
        State = next;
        OnPropertyChanged(nameof(Running));
        OnPropertyChanged(nameof(Busy));
        OnPropertyChanged(nameof(NeedsAttention));
        OnPropertyChanged(nameof(MemoryBytes));
    }

    public void SleepNow()
    {
        if (Chat is { } c)
        {
            c.Sleep();
        }
        else if (TerminalRunning)
        {
            TerminalSleeping = true;
            Terminal?.Terminate();
        }
    }
}

public sealed partial class ClaudeWorkspace : ObservableObject
{
    public ObservableCollection<ClaudeTab> Tabs { get; } = new();

    [ObservableProperty] private ClaudeTab? selectedTab;
    [ObservableProperty] private List<ClaudeProject> projects = new();
    [ObservableProperty] private bool loadingProjects;
    [ObservableProperty] private DateTimeOffset? projectsUpdated;
    [ObservableProperty] private bool showInspector = true;
    [ObservableProperty] private int attentionCount;
    [ObservableProperty] private int runningCount;
    [ObservableProperty] private long totalMemory;

    private readonly Dictionary<string, GitWorkingCopy> gits = new(StringComparer.OrdinalIgnoreCase);
    private Timer? monitor;
    private readonly SynchronizationContext? uiContext;

    public event Action<ClaudeTab>? TabOpened;
    public event Action<ClaudeTab>? AttentionRequested;

    public ClaudeWorkspace()
    {
        uiContext = SynchronizationContext.Current;
        Tabs.CollectionChanged += (_, _) => UpdateCounts();
    }

    private static Settings S => Settings.Shared;

    public ClaudeModelOption Model
    {
        get => ClaudeEnums.ParseModel(S.GetString(Keys.ClaudeModel));
        set
        {
            S.Set(Keys.ClaudeModel, value.Raw());
            OnPropertyChanged();
        }
    }

    public ClaudeEffort Effort
    {
        get => ClaudeEnums.ParseEffort(S.GetString(Keys.ClaudeEffort));
        set
        {
            S.Set(Keys.ClaudeEffort, value.Raw());
            OnPropertyChanged();
        }
    }

    public ClaudeMode Mode
    {
        get => ClaudeEnums.ParseMode(S.GetString(Keys.ClaudeMode));
        set
        {
            S.Set(Keys.ClaudeMode, value.Raw());
            OnPropertyChanged();
        }
    }

    public int IdleMinutes
    {
        get => S.GetInt(Keys.ClaudeIdleMinutes, 15);
        set
        {
            S.Set(Keys.ClaudeIdleMinutes, value);
            OnPropertyChanged();
        }
    }

    public ClaudeTabKind PreferredKind
    {
        get => S.GetString(Keys.ClaudeUi) == "terminal" ? ClaudeTabKind.Terminal : ClaudeTabKind.Chat;
        set
        {
            S.Set(Keys.ClaudeUi, value == ClaudeTabKind.Terminal ? "terminal" : "chat");
            OnPropertyChanged();
        }
    }

    public bool CliAvailable => ClaudeLauncher.Available;

    public void UpdateCounts()
    {
        AttentionCount = Tabs.Count(t => t.NeedsAttention);
        RunningCount = Tabs.Count(t => t.Running);
        TotalMemory = Tabs.Sum(t => t.MemoryBytes);
    }

    private void Ui(Action a)
    {
        if (uiContext is null || SynchronizationContext.Current == uiContext) a();
        else uiContext.Post(_ => a(), null);
    }

    public void StartMonitoring()
    {
        monitor ??= new Timer(_ => Ui(() => Tick().Forget()), null, TimeSpan.FromSeconds(15), TimeSpan.FromSeconds(15));
    }

    public async Task Tick()
    {
        var live = Tabs.Where(t => t.Running).ToList();
        if (live.Count > 0)
        {
            var pids = live.Select(t => t.Pid).ToList();
            var bytes = await Task.Run(() =>
            {
                var map = ProcessMemory.ParentMap();
                return pids.Select(p => ProcessMemory.TreeBytes(p, map)).ToList();
            });
            for (var i = 0; i < live.Count; i++)
            {
                if (live[i].Chat is { } chat) chat.MemoryBytes = bytes[i];
                else live[i].TerminalMemory = bytes[i];
            }
        }
        foreach (var t in Tabs) t.UpdateState();
        UpdateCounts();
        var limit = IdleMinutes;
        if (limit <= 0) return;
        var threshold = TimeSpan.FromMinutes(limit);
        foreach (var tab in Tabs)
        {
            if (tab.Chat is { } chat)
            {
                if (chat.CanSleep && DateTimeOffset.Now - chat.LastActivity > threshold) chat.Sleep();
            }
            else if (tab.TerminalRunning && tab.Terminal is { } term && DateTimeOffset.Now - term.LastOutput > threshold)
            {
                tab.SleepNow();
            }
        }
    }

    public void CloseAllForProfileSwitch()
    {
        foreach (var tab in Tabs.ToList())
        {
            tab.Chat?.Stop();
            tab.Terminal?.Dispose();
        }
        Tabs.Clear();
        SelectedTab = null;
        Projects = new();
    }

    public void SleepAll()
    {
        foreach (var tab in Tabs.Where(t => t.Running && t.State != ClaudeTabState.Working && t.State != ClaudeTabState.Waiting).ToList()) tab.SleepNow();
    }

    public GitWorkingCopy GitFor(string folder)
    {
        if (gits.TryGetValue(folder, out var g)) return g;
        g = new GitWorkingCopy(folder);
        gits[folder] = g;
        g.StartWatching();
        g.Refresh().Forget();
        return g;
    }

    public async Task ReloadProjects()
    {
        LoadingProjects = true;
        try
        {
            var root = Path.Combine(ClaudeProfiles.Shared.ConfigDir(), "projects");
            var profileId = ClaudeProfiles.Shared.ActiveId;
            var loaded = await Task.Run(() => ClaudeSessionParser.LoadAll(root));
            if (profileId != ClaudeProfiles.Shared.ActiveId) return;
            if (Demo.Folder is { } f) loaded = loaded.Where(p => p.Path.StartsWith(f, StringComparison.OrdinalIgnoreCase)).ToList();
            Projects = loaded;
            ProjectsUpdated = DateTimeOffset.Now;
        }
        finally
        {
            LoadingProjects = false;
        }
    }

    public IEnumerable<ClaudeSessionInfo> AllSessions => Projects.SelectMany(p => p.Sessions);

    public ClaudeTab? TabForSession(string id) => Tabs.FirstOrDefault(t => t.SessionId == id);

    public ClaudeTab NewSession(string folder, string? prompt = null, string? name = null, ClaudeMode? mode = null, ClaudeTabKind? kind = null)
    {
        var k = kind ?? PreferredKind;
        var title = name ?? (prompt is { Length: > 0 } p ? (p.Length > 48 ? p[..48] : p) : "New session");
        if (k == ClaudeTabKind.Chat)
        {
            var tab = MakeTab(folder, ClaudeTabKind.Chat, title, "");
            StartChat(tab, null, false, prompt, mode);
            return tab;
        }
        var sessionId = Guid.NewGuid().ToString().ToLowerInvariant();
        var args = new List<string> { "--session-id", sessionId };
        var viaShim = ClaudeLauncher.Command?.UsesCmdShell == true;
        if (!string.IsNullOrEmpty(name)) args.AddRange(new[] { "-n", viaShim ? ClaudeLauncher.SanitizeName(name) : name });
        if (!viaShim && !string.IsNullOrEmpty(prompt)) args.Add(ClaudeLauncher.FitCommandLine(prompt));
        var t = MakeTab(folder, ClaudeTabKind.Terminal, title, sessionId);
        LaunchTerminal(t, args, mode);
        return t;
    }

    public ClaudeTab Resume(ClaudeSessionInfo s, bool fork = false, ClaudeTabKind? kind = null)
    {
        if (!fork && TabForSession(s.Id) is { } open)
        {
            SelectedTab = open;
            return open;
        }
        var k = kind ?? PreferredKind;
        var tab = MakeTab(s.Cwd, k, s.Title, fork ? "" : s.Id);
        if (k == ClaudeTabKind.Chat)
        {
            tab.Chat?.LoadHistory(s.File);
            StartChat(tab, s.Id, fork, null, null);
        }
        else
        {
            var args = new List<string> { "--resume", s.Id };
            if (fork) args.Add("--fork-session");
            LaunchTerminal(tab, args, null);
        }
        return tab;
    }

    public void OpenInTerminal(ClaudeTab tab)
    {
        if (string.IsNullOrEmpty(tab.SessionId)) return;
        tab.Chat?.Stop();
        var t = MakeTab(tab.Folder, ClaudeTabKind.Terminal, tab.Title, tab.SessionId, select: false);
        LaunchTerminal(t, new List<string> { "--resume", tab.SessionId }, null);
        Replace(tab, t);
    }

    public void OpenInChat(ClaudeTab tab)
    {
        if (string.IsNullOrEmpty(tab.SessionId)) return;
        tab.Terminal?.Dispose();
        var c = MakeTab(tab.Folder, ClaudeTabKind.Chat, tab.Title, tab.SessionId, select: false);
        if (AllSessions.FirstOrDefault(s => s.Id == tab.SessionId) is { } info) c.Chat?.LoadHistory(info.File);
        StartChat(c, tab.SessionId, false, null, null);
        Replace(tab, c);
    }

    private void Replace(ClaudeTab old, ClaudeTab next)
    {
        var index = Tabs.IndexOf(old);
        Tabs.Remove(next);
        if (index >= 0)
        {
            Tabs.RemoveAt(index);
            Tabs.Insert(Math.Min(index, Tabs.Count), next);
        }
        else
        {
            Tabs.Add(next);
        }
        SelectedTab = next;
    }

    public void Restart(ClaudeTab tab)
    {
        if (tab.Running) return;
        if (tab.Kind == ClaudeTabKind.Chat)
        {
            if (tab.Chat is { } chat && !string.IsNullOrEmpty(chat.SessionId)) chat.Wake();
            else StartChat(tab, string.IsNullOrEmpty(tab.SessionId) ? null : tab.SessionId, false, null, null);
        }
        else
        {
            tab.TerminalSleeping = false;
            LaunchTerminal(tab, new List<string> { "--resume", tab.SessionId }, null);
        }
    }

    public void Close(ClaudeTab tab)
    {
        tab.Chat?.Stop();
        tab.Terminal?.Dispose();
        tab.Terminal = null;
        var wasSelected = SelectedTab == tab;
        Tabs.Remove(tab);
        if (wasSelected) SelectedTab = Tabs.LastOrDefault();
    }

    public void ApplyModelToCurrent()
    {
        if (SelectedTab is not { Running: true } tab || Model == ClaudeModelOption.Standard) return;
        if (tab.Chat is { } chat) chat.SetModel(Model.Raw());
        else tab.Terminal?.Write($"/model {Model.Raw()}\r");
    }

    public void Send(string text, ClaudeTab? target = null, bool submit = false)
    {
        if ((target ?? SelectedTab) is not { } tab) return;
        if (tab.Chat is { } chat)
        {
            if (!chat.Running) Restart(tab);
            chat.Send(text);
            return;
        }
        if (!tab.Running) return;
        tab.Terminal?.Write("\u001b[200~" + text + "\u001b[201~");
        if (submit) tab.Terminal?.Write("\r");
    }

    private ClaudeTab MakeTab(string folder, ClaudeTabKind kind, string title, string sessionId, bool select = true)
    {
        var tab = new ClaudeTab(folder, kind, title, sessionId, GitFor(folder));
        tab.PropertyChanged += (_, e) =>
        {
            if (e.PropertyName is nameof(ClaudeTab.State) or nameof(ClaudeTab.MemoryBytes)) UpdateCounts();
        };
        if (tab.Chat is { } chat)
        {
            chat.AttentionRequested += () => AttentionRequested?.Invoke(tab);
        }
        Tabs.Add(tab);
        if (select) SelectedTab = tab;
        StartMonitoring();
        TabOpened?.Invoke(tab);
        Task.Delay(4000).ContinueWith(_ => Ui(() => _ = ReloadProjects()), TaskScheduler.Default).Forget();
        return tab;
    }

    private void StartChat(ClaudeTab tab, string? resume, bool fork, string? prompt, ClaudeMode? modeOverride)
    {
        if (tab.Chat is not { } chat) return;
        chat.OnSessionId += sid => tab.SessionId = sid;
        chat.OnTurnFinished += () => tab.Git.Refresh().Forget();
        var m = modeOverride ?? Mode;
        chat.Start(resume, fork,
            Model == ClaudeModelOption.Standard ? null : Model.Raw(),
            Effort == ClaudeEffort.Standard ? null : Effort.Raw(),
            m == ClaudeMode.Standard ? null : m.Raw(),
            prompt);
        tab.UpdateState();
    }

    public List<string> TerminalArgs(IEnumerable<string> extra, ClaudeMode? modeOverride)
    {
        var mode = modeOverride ?? Mode;
        var args = new List<string>();
        if (Model != ClaudeModelOption.Standard) args.AddRange(new[] { "--model", Model.Raw() });
        if (Effort != ClaudeEffort.Standard) args.AddRange(new[] { "--effort", Effort.Raw() });
        if (mode != ClaudeMode.Standard) args.AddRange(new[] { "--permission-mode", mode.Raw() });
        args.AddRange(extra);
        return args;
    }

    private void LaunchTerminal(ClaudeTab tab, List<string> extra, ClaudeMode? modeOverride)
    {
        if (ClaudeLauncher.Command is not { } cmd)
        {
            tab.ExitCode = -1;
            return;
        }
        var args = TerminalArgs(extra, modeOverride);
        var env = ClaudeLauncher.ProcessEnvironment(tab.ConfigDir);
        env["COLORTERM"] = "truecolor";
        env["TERM"] = "xterm-256color";
        tab.Terminal?.Dispose();
        var session = new TerminalSession();
        tab.Terminal = session;
        tab.ExitCode = null;
        tab.StartedAt = DateTimeOffset.Now;
        session.Exited += code => Ui(() =>
        {
            if (!ReferenceEquals(tab.Terminal, session)) return;
            tab.TerminalRunning = false;
            tab.ExitCode = code;
        });
        try
        {
            session.Start(cmd.CommandLine(args), tab.Folder, env, 120, 32);
            tab.TerminalRunning = true;
        }
        catch (Exception)
        {
            tab.TerminalRunning = false;
            tab.ExitCode = -1;
        }
    }
}
