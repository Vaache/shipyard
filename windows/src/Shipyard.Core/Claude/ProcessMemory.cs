using System.Diagnostics;
using System.Runtime.InteropServices;

namespace Shipyard.Core.Claude;

public static class ProcessMemory
{
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct ProcessEntry32
    {
        public uint dwSize;
        public uint cntUsage;
        public uint th32ProcessID;
        public IntPtr th32DefaultHeapID;
        public uint th32ModuleID;
        public uint cntThreads;
        public uint th32ParentProcessID;
        public int pcPriClassBase;
        public uint dwFlags;

        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)]
        public string szExeFile;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint processId);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool Process32FirstW(IntPtr snapshot, ref ProcessEntry32 entry);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool Process32NextW(IntPtr snapshot, ref ProcessEntry32 entry);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool CloseHandle(IntPtr handle);

    public static Dictionary<int, int> ParentMap()
    {
        var map = new Dictionary<int, int>();
        if (!OperatingSystem.IsWindows()) return map;
        var snap = CreateToolhelp32Snapshot(0x2, 0);
        if (snap == IntPtr.Zero || snap == new IntPtr(-1)) return map;
        try
        {
            var entry = new ProcessEntry32 { dwSize = (uint)Marshal.SizeOf<ProcessEntry32>() };
            if (!Process32FirstW(snap, ref entry)) return map;
            do
            {
                map[(int)entry.th32ProcessID] = (int)entry.th32ParentProcessID;
            } while (Process32NextW(snap, ref entry));
        }
        finally
        {
            CloseHandle(snap);
        }
        return map;
    }

    public static long TreeBytes(int root, Dictionary<int, int> parents)
    {
        if (root <= 0) return 0;
        var children = new Dictionary<int, List<int>>();
        foreach (var (pid, ppid) in parents)
        {
            if (!children.TryGetValue(ppid, out var l)) children[ppid] = l = new();
            l.Add(pid);
        }
        long total = 0;
        var stack = new Stack<int>();
        stack.Push(root);
        var seen = new HashSet<int>();
        while (stack.Count > 0)
        {
            var pid = stack.Pop();
            if (!seen.Add(pid)) continue;
            try
            {
                using var p = Process.GetProcessById(pid);
                total += p.WorkingSet64;
            }
            catch
            {
            }
            if (children.TryGetValue(pid, out var kids))
                foreach (var k in kids) stack.Push(k);
        }
        return total;
    }
}
