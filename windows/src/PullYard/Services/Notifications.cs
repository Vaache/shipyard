using Microsoft.Windows.AppNotifications;
using Microsoft.Windows.AppNotifications.Builder;

namespace PullYard.Services;

public static class Notifications
{
    private static bool registered;

    public static event Action<string>? Invoked;

    public static void Register()
    {
        if (registered) return;
        try
        {
            AppNotificationManager.Default.NotificationInvoked += (_, args) =>
            {
                var action = args.Arguments.TryGetValue("action", out var a) ? a : "";
                Invoked?.Invoke(action);
            };
            AppNotificationManager.Default.Register();
            registered = true;
        }
        catch
        {
        }
    }

    public static void Show(string title, string body, string action)
    {
        if (!registered) return;
        try
        {
            var n = new AppNotificationBuilder()
                .AddArgument("action", action)
                .AddText(title)
                .AddText(body)
                .BuildNotification();
            AppNotificationManager.Default.Show(n);
        }
        catch
        {
        }
    }

    public static void Unregister()
    {
        if (!registered) return;
        try
        {
            AppNotificationManager.Default.Unregister();
        }
        catch
        {
        }
    }
}
