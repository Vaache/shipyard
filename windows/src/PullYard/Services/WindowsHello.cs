using PullYard.Core.Util;
using Windows.Security.Credentials.UI;

namespace PullYard.Services;

public static class WindowsHello
{
    public static bool Available { get; private set; }

    public static async Task Refresh()
    {
        try
        {
            Available = await UserConsentVerifier.CheckAvailabilityAsync() == UserConsentVerifierAvailability.Available;
        }
        catch (Exception e)
        {
            ErrorLog.Write(e, "Windows Hello availability check failed");
            Available = false;
        }
    }

    public static async Task<bool> Verify(string message)
    {
        try
        {
            var hwnd = App.MainWindow is { } window ? WindowInterop.Handle(window) : IntPtr.Zero;
            var result = hwnd != IntPtr.Zero
                ? await UserConsentVerifierInterop.RequestVerificationForWindowAsync(hwnd, message)
                : await UserConsentVerifier.RequestVerificationAsync(message);
            return result == UserConsentVerificationResult.Verified;
        }
        catch (Exception e)
        {
            ErrorLog.Write(e, "Windows Hello verification failed");
            return false;
        }
    }
}
