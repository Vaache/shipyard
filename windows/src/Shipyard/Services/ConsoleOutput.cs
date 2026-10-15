using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace Shipyard.Services;

public static class ConsoleOutput
{
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr GetStdHandle(int nStdHandle);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool AttachConsole(int dwProcessId);

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern SafeFileHandle CreateFileW(string name, uint access, uint share, IntPtr security, uint creation, uint flags, IntPtr template);

    public static void Attach()
    {
        var handle = GetStdHandle(-11);
        if (handle != IntPtr.Zero && handle != new IntPtr(-1))
        {
            var writer = new StreamWriter(Console.OpenStandardOutput()) { AutoFlush = true };
            Console.SetOut(writer);
            Console.SetError(writer);
            return;
        }
        if (!AttachConsole(-1)) return;
        var conout = CreateFileW("CONOUT$", 0x40000000, 0x2, IntPtr.Zero, 3, 0, IntPtr.Zero);
        if (conout.IsInvalid) return;
        var stream = new FileStream(conout, FileAccess.Write);
        var console = new StreamWriter(stream) { AutoFlush = true };
        Console.SetOut(console);
        Console.SetError(console);
    }
}
