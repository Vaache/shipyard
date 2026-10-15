using PullYard.Core.Util;

namespace PullYard;

public static class TaskExtensions
{
    public static void Forget(this Task task)
    {
        task.ContinueWith(t =>
        {
            if (t.Exception is { } e) ErrorLog.Write(e.Flatten(), "Unobserved task failure");
        }, TaskContinuationOptions.OnlyOnFaulted | TaskContinuationOptions.ExecuteSynchronously);
    }
}
