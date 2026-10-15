using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;

namespace Shipyard.Core.Claude;

public sealed class TerminalSession : IDisposable
{
    [StructLayout(LayoutKind.Sequential)]
    private struct Coord
    {
        public short X;
        public short Y;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct StartupInfo
    {
        public int cb;
        public string? lpReserved;
        public string? lpDesktop;
        public string? lpTitle;
        public int dwX;
        public int dwY;
        public int dwXSize;
        public int dwYSize;
        public int dwXCountChars;
        public int dwYCountChars;
        public int dwFillAttribute;
        public int dwFlags;
        public short wShowWindow;
        public short cbReserved2;
        public IntPtr lpReserved2;
        public IntPtr hStdInput;
        public IntPtr hStdOutput;
        public IntPtr hStdError;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct StartupInfoEx
    {
        public StartupInfo StartupInfo;
        public IntPtr lpAttributeList;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct ProcessInformation
    {
        public IntPtr hProcess;
        public IntPtr hThread;
        public int dwProcessId;
        public int dwThreadId;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct SecurityAttributes
    {
        public int nLength;
        public IntPtr lpSecurityDescriptor;
        public int bInheritHandle;
    }

    private const uint ExtendedStartupInfoPresent = 0x00080000;
    private const uint CreateUnicodeEnvironment = 0x00000400;
    private static readonly IntPtr ProcThreadAttributePseudoConsole = (IntPtr)0x00020016;
    private const int StartfUseStdHandles = 0x00000100;

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern int CreatePseudoConsole(Coord size, SafeFileHandle hInput, SafeFileHandle hOutput, uint dwFlags, out IntPtr phPC);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern int ResizePseudoConsole(IntPtr hPC, Coord size);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern void ClosePseudoConsole(IntPtr hPC);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool CreatePipe(out SafeFileHandle hReadPipe, out SafeFileHandle hWritePipe, ref SecurityAttributes lpPipeAttributes, int nSize);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool InitializeProcThreadAttributeList(IntPtr lpAttributeList, int dwAttributeCount, int dwFlags, ref IntPtr lpSize);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool UpdateProcThreadAttribute(IntPtr lpAttributeList, uint dwFlags, IntPtr attribute, IntPtr lpValue, IntPtr cbSize, IntPtr lpPreviousValue, IntPtr lpReturnSize);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern void DeleteProcThreadAttributeList(IntPtr lpAttributeList);

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern bool CreateProcessW(string? lpApplicationName, StringBuilder lpCommandLine, IntPtr lpProcessAttributes, IntPtr lpThreadAttributes, bool bInheritHandles,
        uint dwCreationFlags, IntPtr lpEnvironment, string? lpCurrentDirectory, ref StartupInfoEx lpStartupInfo, out ProcessInformation lpProcessInformation);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint WaitForSingleObject(IntPtr hHandle, uint dwMilliseconds);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetExitCodeProcess(IntPtr hProcess, out uint lpExitCode);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool TerminateProcess(IntPtr hProcess, uint uExitCode);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool CloseHandle(IntPtr handle);

    private IntPtr pseudoConsole;
    private SafeFileHandle? inputWrite;
    private SafeFileHandle? outputRead;
    private FileStream? inputStream;
    private ProcessInformation info;
    private IntPtr attributeList;
    private bool disposed;
    private readonly object gate = new();

    private Action<byte[]>? sink;
    private readonly MemoryStream history = new();
    private const int HistoryLimit = 4 * 1024 * 1024;

    public event Action<int>? Exited;

    public byte[] Attach(Action<byte[]> handler)
    {
        lock (gate)
        {
            sink = handler;
            return history.ToArray();
        }
    }

    public void Detach(Action<byte[]> handler)
    {
        lock (gate)
        {
            if (sink == handler) sink = null;
        }
    }

    public int ProcessId => info.dwProcessId;
    public bool IsRunning { get; private set; }
    public DateTimeOffset LastOutput { get; private set; } = DateTimeOffset.Now;

    public void Start(string commandLine, string workingDirectory, IDictionary<string, string?> environment, int cols, int rows)
    {
        if (!OperatingSystem.IsWindows()) throw new PlatformNotSupportedException("ConPTY requires Windows");
        var sa = new SecurityAttributes { nLength = Marshal.SizeOf<SecurityAttributes>(), bInheritHandle = 1 };
        if (!CreatePipe(out var inputRead, out var inWrite, ref sa, 0)) throw new Win32Exception(Marshal.GetLastWin32Error());
        if (!CreatePipe(out var outRead, out var outputWrite, ref sa, 0)) throw new Win32Exception(Marshal.GetLastWin32Error());
        var hr = CreatePseudoConsole(new Coord { X = (short)Math.Max(cols, 20), Y = (short)Math.Max(rows, 5) }, inputRead, outputWrite, 0, out pseudoConsole);
        if (hr != 0) throw new Win32Exception(hr);
        inputRead.Dispose();
        outputWrite.Dispose();
        inputWrite = inWrite;
        outputRead = outRead;

        var size = IntPtr.Zero;
        InitializeProcThreadAttributeList(IntPtr.Zero, 1, 0, ref size);
        attributeList = Marshal.AllocHGlobal(size);
        if (!InitializeProcThreadAttributeList(attributeList, 1, 0, ref size)) throw new Win32Exception(Marshal.GetLastWin32Error());
        if (!UpdateProcThreadAttribute(attributeList, 0, ProcThreadAttributePseudoConsole, pseudoConsole, (IntPtr)IntPtr.Size, IntPtr.Zero, IntPtr.Zero))
            throw new Win32Exception(Marshal.GetLastWin32Error());

        var startup = new StartupInfoEx { lpAttributeList = attributeList };
        startup.StartupInfo.cb = Marshal.SizeOf<StartupInfoEx>();
        startup.StartupInfo.dwFlags = StartfUseStdHandles;

        var envBlock = BuildEnvironment(environment);
        var envPtr = Marshal.StringToHGlobalUni(envBlock);
        try
        {
            var cmd = new StringBuilder(commandLine);
            if (!CreateProcessW(null, cmd, IntPtr.Zero, IntPtr.Zero, false, ExtendedStartupInfoPresent | CreateUnicodeEnvironment, envPtr,
                    Directory.Exists(workingDirectory) ? workingDirectory : null, ref startup, out info))
                throw new Win32Exception(Marshal.GetLastWin32Error());
        }
        finally
        {
            Marshal.FreeHGlobal(envPtr);
        }
        IsRunning = true;
        inputStream = new FileStream(inputWrite, FileAccess.Write, 1, false);
        var reader = new Thread(ReadLoop) { IsBackground = true, Name = "conpty-read" };
        reader.Start();
        var waiter = new Thread(WaitLoop) { IsBackground = true, Name = "conpty-wait" };
        waiter.Start();
    }

    private static string BuildEnvironment(IDictionary<string, string?> overrides)
    {
        var env = new SortedDictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        foreach (System.Collections.DictionaryEntry e in Environment.GetEnvironmentVariables())
            env[(string)e.Key] = (string?)e.Value ?? "";
        foreach (var (k, v) in overrides)
        {
            if (v is null) env.Remove(k);
            else env[k] = v;
        }
        var sb = new StringBuilder();
        foreach (var (k, v) in env)
        {
            if (k.Length == 0 || k.Contains('=') && !k.StartsWith('=')) continue;
            sb.Append(k).Append('=').Append(v).Append('\0');
        }
        sb.Append('\0');
        return sb.ToString();
    }

    private void ReadLoop()
    {
        try
        {
            using var stream = new FileStream(outputRead!, FileAccess.Read, 1, false);
            var buffer = new byte[16 * 1024];
            while (true)
            {
                int n;
                try
                {
                    n = stream.Read(buffer, 0, buffer.Length);
                }
                catch
                {
                    break;
                }
                if (n <= 0) break;
                LastOutput = DateTimeOffset.Now;
                var chunk = new byte[n];
                Buffer.BlockCopy(buffer, 0, chunk, 0, n);
                lock (gate)
                {
                    if (history.Length + n > HistoryLimit)
                    {
                        var keep = history.ToArray();
                        var start = keep.Length / 2;
                        history.SetLength(0);
                        history.Write(keep, start, keep.Length - start);
                    }
                    history.Write(chunk, 0, n);
                    sink?.Invoke(chunk);
                }
            }
        }
        catch
        {
        }
    }

    private void WaitLoop()
    {
        WaitForSingleObject(info.hProcess, 0xFFFFFFFF);
        GetExitCodeProcess(info.hProcess, out var code);
        IsRunning = false;
        lock (gate)
        {
            if (pseudoConsole != IntPtr.Zero)
            {
                ClosePseudoConsole(pseudoConsole);
                pseudoConsole = IntPtr.Zero;
            }
        }
        Exited?.Invoke((int)code);
    }

    public void Write(string text) => Write(Encoding.UTF8.GetBytes(text));

    public void Write(byte[] data)
    {
        if (!IsRunning || inputStream is null) return;
        try
        {
            inputStream.Write(data, 0, data.Length);
            inputStream.Flush();
        }
        catch
        {
        }
    }

    public void Resize(int cols, int rows)
    {
        lock (gate)
        {
            if (pseudoConsole == IntPtr.Zero || cols <= 0 || rows <= 0) return;
            ResizePseudoConsole(pseudoConsole, new Coord { X = (short)cols, Y = (short)rows });
        }
    }

    public void Terminate()
    {
        if (!IsRunning) return;
        try
        {
            TerminateProcess(info.hProcess, 1);
        }
        catch
        {
        }
    }

    public void Dispose()
    {
        if (disposed) return;
        disposed = true;
        Terminate();
        lock (gate)
        {
            if (pseudoConsole != IntPtr.Zero)
            {
                ClosePseudoConsole(pseudoConsole);
                pseudoConsole = IntPtr.Zero;
            }
        }
        inputStream?.Dispose();
        if (attributeList != IntPtr.Zero)
        {
            DeleteProcThreadAttributeList(attributeList);
            Marshal.FreeHGlobal(attributeList);
            attributeList = IntPtr.Zero;
        }
        if (info.hProcess != IntPtr.Zero) CloseHandle(info.hProcess);
        if (info.hThread != IntPtr.Zero) CloseHandle(info.hThread);
        info = default;
    }
}
