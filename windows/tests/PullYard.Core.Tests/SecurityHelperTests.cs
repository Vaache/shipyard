using PullYard.Core.Claude;
using PullYard.Core.Util;
using Xunit;

namespace PullYard.Core.Tests;

public class SecurityHelperTests
{
    [Theory]
    [InlineData("Fix PR #12: Update \"parser\" & more", "Fix PR #12  Update  parser    more")]
    [InlineData("a;b|c%PATH%\r\nd", "a b c PATH   d")]
    [InlineData("Plain name-1.0_x", "Plain name-1.0_x")]
    [InlineData("", "Claude")]
    [InlineData("&&&", "Claude")]
    public void SanitizesSessionNames(string input, string expected)
    {
        Assert.Equal(expected, ClaudeLauncher.SanitizeName(input));
    }

    [Fact]
    public void SanitizedNamesNeverContainShellMetacharacters()
    {
        var clean = ClaudeLauncher.SanitizeName("x\";&|<>^%!`$(){}[]'\\\n");
        Assert.DoesNotContain(clean, c => ";&|<>^%!`$(){}[]'\"\\\n".Contains(c));
    }

    [Fact]
    public void DetectsPathsInsideWorkspace()
    {
        var root = Path.Combine(Path.GetTempPath(), "pullyard-ws");
        Assert.True(PathGuard.IsInside(root, Path.Combine(root, "src", "a.txt")));
        Assert.False(PathGuard.IsInside(root, Path.Combine(root, "..", "other", "a.txt")));
        Assert.False(PathGuard.IsInside(root, root + "-sibling"));
        Assert.False(PathGuard.IsInside(root, ""));
    }

    [Theory]
    [InlineData("a.exe", true)]
    [InlineData("A.BAT", true)]
    [InlineData("x.ps1", true)]
    [InlineData("x.lnk", true)]
    [InlineData("notes.txt", false)]
    [InlineData("image.png", false)]
    [InlineData("Makefile", false)]
    public void FlagsExecutableExtensions(string path, bool expected)
    {
        Assert.Equal(expected, PathGuard.IsExecutableExtension(path));
    }
}
