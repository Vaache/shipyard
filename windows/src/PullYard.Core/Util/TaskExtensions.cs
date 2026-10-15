namespace PullYard;

public static class TaskExtensions
{
    public static void Forget(this Task task)
    {
        task.ContinueWith(t => _ = t.Exception, TaskContinuationOptions.OnlyOnFaulted | TaskContinuationOptions.ExecuteSynchronously);
    }
}
