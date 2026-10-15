using System.Diagnostics;
using System.Text;

namespace PullYard.Core.Util;

public static class Proc
{
    public sealed record Result(string Out, string Err, int Code)
    {
        public bool Ok => Code == 0;
    }

    public static async Task<Result> Run(string file, IEnumerable<string> args, string? cwd = null, IDictionary<string, string?>? env = null, string? input = null, TimeSpan? timeout = null, CancellationToken ct = default)
    {
        var psi = new ProcessStartInfo(file)
        {
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            RedirectStandardInput = input is not null,
            UseShellExecute = false,
            CreateNoWindow = true,
            StandardOutputEncoding = Encoding.UTF8,
            StandardErrorEncoding = Encoding.UTF8
        };
        foreach (var a in args) psi.ArgumentList.Add(a);
        if (cwd is not null) psi.WorkingDirectory = cwd;
        if (env is not null)
            foreach (var (k, v) in env)
            {
                if (v is null) psi.Environment.Remove(k);
                else psi.Environment[k] = v;
            }
        using var p = new Process { StartInfo = psi };
        try
        {
            if (!p.Start()) return new Result("", "failed to start", -1);
        }
        catch (Exception e)
        {
            return new Result("", e.Message, -1);
        }
        if (input is not null)
        {
            await p.StandardInput.WriteAsync(input).ConfigureAwait(false);
            p.StandardInput.Close();
        }
        var outTask = p.StandardOutput.ReadToEndAsync();
        var errTask = p.StandardError.ReadToEndAsync();
        using var cts = CancellationTokenSource.CreateLinkedTokenSource(ct);
        if (timeout is { } t) cts.CancelAfter(t);
        try
        {
            await p.WaitForExitAsync(cts.Token).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            try
            {
                p.Kill(true);
            }
            catch
            {
            }
            return new Result(await SafeAwait(outTask), "timed out", -1);
        }
        return new Result(await outTask.ConfigureAwait(false), await errTask.ConfigureAwait(false), p.ExitCode);
    }

    private static async Task<string> SafeAwait(Task<string> t)
    {
        try
        {
            return await t.ConfigureAwait(false);
        }
        catch
        {
            return "";
        }
    }

    public static string? FindOnPath(params string[] names)
    {
        var path = Environment.GetEnvironmentVariable("PATH") ?? "";
        var exts = OperatingSystem.IsWindows()
            ? (Environment.GetEnvironmentVariable("PATHEXT") ?? ".EXE;.CMD;.BAT").Split(';', StringSplitOptions.RemoveEmptyEntries)
            : new[] { "" };
        foreach (var dir in path.Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries))
        {
            foreach (var name in names)
            {
                if (Path.HasExtension(name))
                {
                    var full = Path.Combine(dir.Trim('"'), name);
                    if (File.Exists(full)) return full;
                    continue;
                }
                foreach (var ext in exts)
                {
                    var full = Path.Combine(dir.Trim('"'), name + ext.ToLowerInvariant());
                    if (File.Exists(full)) return full;
                }
            }
        }
        return null;
    }

    public static void OpenUrl(string url)
    {
        try
        {
            Process.Start(new ProcessStartInfo(url) { UseShellExecute = true });
        }
        catch
        {
        }
    }

    public static void Reveal(string path)
    {
        try
        {
            if (OperatingSystem.IsWindows())
            {
                if (File.Exists(path)) Process.Start(new ProcessStartInfo("explorer.exe", $"/select,\"{path}\"") { UseShellExecute = false });
                else Process.Start(new ProcessStartInfo("explorer.exe", $"\"{path}\"") { UseShellExecute = false });
            }
            else
            {
                Process.Start(new ProcessStartInfo(path) { UseShellExecute = true });
            }
        }
        catch
        {
        }
    }

    public static string QuoteArg(string arg)
    {
        if (arg.Length > 0 && arg.IndexOfAny(new[] { ' ', '\t', '"', '\n' }) < 0) return arg;
        var sb = new StringBuilder("\"");
        var backslashes = 0;
        foreach (var c in arg)
        {
            if (c == '\\')
            {
                backslashes++;
                continue;
            }
            if (c == '"')
            {
                sb.Append('\\', backslashes * 2 + 1).Append('"');
            }
            else
            {
                sb.Append('\\', backslashes).Append(c);
            }
            backslashes = 0;
        }
        sb.Append('\\', backslashes * 2).Append('"');
        return sb.ToString();
    }
}
