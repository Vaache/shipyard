using System.Runtime.InteropServices;
using System.Text;
using PullYard.Core;
using PullYard.Core.Auth;
using PullYard.Core.Util;

namespace PullYard.Services;

public static class CredentialStore
{
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct Credential
    {
        public uint Flags;
        public uint Type;
        public string TargetName;
        public string? Comment;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
        public uint CredentialBlobSize;
        public IntPtr CredentialBlob;
        public uint Persist;
        public uint AttributeCount;
        public IntPtr Attributes;
        public string? TargetAlias;
        public string? UserName;
    }

    private const uint CredTypeGeneric = 1;
    private const uint CredPersistLocalMachine = 2;

    [DllImport("advapi32.dll", EntryPoint = "CredReadW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CredRead(string target, uint type, int flags, out IntPtr credential);

    [DllImport("advapi32.dll", EntryPoint = "CredWriteW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CredWrite(ref Credential credential, uint flags);

    [DllImport("advapi32.dll", EntryPoint = "CredDeleteW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CredDelete(string target, uint type, int flags);

    [DllImport("advapi32.dll")]
    private static extern void CredFree(IntPtr buffer);

    private static string Target(string key) => $"{AppInfo.DisplayName}:{key}";

    public static void Save(string token, string key = GitAccounts.DefaultCredentialKey)
    {
        var bytes = Encoding.Unicode.GetBytes(token);
        var blob = Marshal.AllocHGlobal(bytes.Length);
        try
        {
            Marshal.Copy(bytes, 0, blob, bytes.Length);
            var cred = new Credential
            {
                Type = CredTypeGeneric,
                TargetName = Target(key),
                CredentialBlobSize = (uint)bytes.Length,
                CredentialBlob = blob,
                Persist = CredPersistLocalMachine,
                UserName = key,
                Comment = $"{AppInfo.DisplayName} sign-in token"
            };
            if (!CredWrite(ref cred, 0))
                throw new IOException($"Couldn't save the token to Windows Credential Manager (error {Marshal.GetLastWin32Error()}). You would be signed out on next launch.");
        }
        finally
        {
            Marshal.FreeHGlobal(blob);
        }
    }

    public static string? Load(string key = GitAccounts.DefaultCredentialKey)
    {
        if (!CredRead(Target(key), CredTypeGeneric, 0, out var ptr)) return null;
        try
        {
            var cred = Marshal.PtrToStructure<Credential>(ptr);
            if (cred.CredentialBlobSize == 0 || cred.CredentialBlob == IntPtr.Zero) return null;
            var bytes = new byte[cred.CredentialBlobSize];
            Marshal.Copy(cred.CredentialBlob, bytes, 0, bytes.Length);
            var token = Encoding.Unicode.GetString(bytes);
            return string.IsNullOrWhiteSpace(token) ? null : token;
        }
        finally
        {
            CredFree(ptr);
        }
    }

    private const int ErrorNotFound = 1168;

    public static bool Delete(string key = GitAccounts.DefaultCredentialKey)
    {
        if (CredDelete(Target(key), CredTypeGeneric, 0)) return true;
        var error = Marshal.GetLastWin32Error();
        if (error == ErrorNotFound) return true;
        ErrorLog.Write($"CredDelete failed with error {error}");
        return false;
    }
}

public sealed class CredentialVault : ICredentialVault
{
    public string? Load(string key) => CredentialStore.Load(key);

    public void Save(string key, string value) => CredentialStore.Save(value, key);

    public bool Delete(string key) => CredentialStore.Delete(key);
}
