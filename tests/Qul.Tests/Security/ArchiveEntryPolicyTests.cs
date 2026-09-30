using System;
using System.Collections.Generic;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Domain.Security;

namespace Qul.Tests.Security;

/// <summary>
/// 路径穿越判定规则的穷举测试。
/// 规则是纯字符串逻辑，所以能被穷举；把安全规则和文件操作混在一起，是这类漏洞最常见的成因。
/// </summary>
[TestClass]
public sealed class ArchiveEntryPolicyTests
{
    private const string Root = @"C:\qul-extract";

    [TestMethod]
    public void NormalEntries_AreAllowed()
    {
        foreach (string name in new[]
                 {
                     "file.txt",
                     "dir/file.txt",
                     "dir\\file.txt",
                     "a/b/c/d.txt",
                     "a//b.txt",
                     "foo/./bar.txt",
                     "中文 目录/文件.txt",
                 })
        {
            Assert.AreEqual(
                ArchivePathVerdict.Allow,
                ArchiveEntryPolicy.Evaluate(name, Root),
                name + " 应当被允许");
        }
    }

    [TestMethod]
    public void ParentTraversal_IsRejected()
    {
        foreach (string name in new[]
                 {
                     "../evil.txt",
                     "..\\evil.txt",
                     "a/../../evil.txt",
                     "a/b/../../../evil.txt",
                     "dir/..",
                     "..",
                 })
        {
            Assert.AreEqual(
                ArchivePathVerdict.ParentTraversal,
                ArchiveEntryPolicy.Evaluate(name, Root),
                name + " 必须被判为路径穿越");
        }
    }

    [TestMethod]
    public void AbsolutePaths_AreRejected()
    {
        Assert.AreEqual(ArchivePathVerdict.AbsolutePath, ArchiveEntryPolicy.Evaluate("/etc/passwd", Root));
        Assert.AreEqual(ArchivePathVerdict.AbsolutePath, ArchiveEntryPolicy.Evaluate("\\Windows\\System32\\x.dll", Root));
        Assert.AreEqual(ArchivePathVerdict.AbsolutePath, ArchiveEntryPolicy.Evaluate("\\\\server\\share\\x", Root));
        Assert.AreEqual(ArchivePathVerdict.AbsolutePath, ArchiveEntryPolicy.Evaluate("/", Root));
    }

    [TestMethod]
    public void DriveLettersAndColons_AreRejected()
    {
        Assert.AreEqual(ArchivePathVerdict.DriveLetter, ArchiveEntryPolicy.Evaluate(@"C:\Windows\x.dll", Root));
        Assert.AreEqual(ArchivePathVerdict.DriveLetter, ArchiveEntryPolicy.Evaluate("C:/Windows/x.dll", Root));
        Assert.AreEqual(ArchivePathVerdict.DriveLetter, ArchiveEntryPolicy.Evaluate("a/C:/b.txt", Root));
        Assert.AreEqual(ArchivePathVerdict.DriveLetter, ArchiveEntryPolicy.Evaluate("a/b:c.txt", Root));
    }

    [TestMethod]
    public void EmptyNames_AreRejected()
    {
        Assert.AreEqual(ArchivePathVerdict.EmptyName, ArchiveEntryPolicy.Evaluate(null, Root));
        Assert.AreEqual(ArchivePathVerdict.EmptyName, ArchiveEntryPolicy.Evaluate(string.Empty, Root));
        Assert.AreEqual(ArchivePathVerdict.EmptyName, ArchiveEntryPolicy.Evaluate("   ", Root));
        Assert.AreEqual(ArchivePathVerdict.EmptyName, ArchiveEntryPolicy.Evaluate("./", Root));
    }

    [TestMethod]
    public void SymbolicLinkEntries_AreRejected()
    {
        Assert.AreEqual(
            ArchivePathVerdict.SymbolicLink,
            ArchiveEntryPolicy.Evaluate("harmless.txt", Root, isSymbolicLink: true),
            "软链条目可以把写入引到目录之外，必须拒绝");
    }

    [TestMethod]
    public void OverlongPaths_AreRejected()
    {
        // 单段 70 字符 × 4 段，加上根路径后合计超过 240 的保守上限。
        string deep = string.Join(
            "/",
            new string('d', 70),
            new string('e', 70),
            new string('f', 70),
            new string('g', 70),
            "x.txt");

        Assert.AreEqual(ArchivePathVerdict.TooLong, ArchiveEntryPolicy.Evaluate(deep, Root));
    }

    [TestMethod]
    public void MissingRoot_IsRejected()
    {
        // 没有受控根目录就谈不上"安全解压"。
        Assert.AreEqual(ArchivePathVerdict.OutsideTargetRoot, ArchiveEntryPolicy.Evaluate("a.txt", string.Empty));
        Assert.AreEqual(ArchivePathVerdict.OutsideTargetRoot, ArchiveEntryPolicy.Evaluate("a.txt", null!));
    }

    [TestMethod]
    public void ResolvedTarget_IsAlwaysUnderRoot()
    {
        string? resolved = ArchiveEntryPolicy.TryResolveTarget(
            "dir/sub/file.txt", Root, out ArchivePathVerdict verdict);

        Assert.AreEqual(ArchivePathVerdict.Allow, verdict);
        Assert.IsNotNull(resolved);
        StringAssert.StartsWith(resolved!, Root + "\\", StringComparison.OrdinalIgnoreCase);

        Assert.IsNull(
            ArchiveEntryPolicy.TryResolveTarget("../evil.txt", Root, out ArchivePathVerdict rejected),
            "判定不通过时不得给出目标路径");
        Assert.AreEqual(ArchivePathVerdict.ParentTraversal, rejected);
    }

    [TestMethod]
    public void TrailingSeparatorDoesNotChangeTheVerdict()
    {
        // 目录条目通常以 '/' 结尾。
        Assert.AreEqual(ArchivePathVerdict.Allow, ArchiveEntryPolicy.Evaluate("dir/", Root));
        Assert.AreEqual(ArchivePathVerdict.ParentTraversal, ArchiveEntryPolicy.Evaluate("../", Root));
    }

    [TestMethod]
    public void Sha1Hex_ParsesAndComparesCaseInsensitively()
    {
        const string upper = "0A796914D1C8A55B4DA9F4A8856DD9623375D8BB";
        const string lower = "0a796914d1c8a55b4da9f4a8856dd9623375d8bb";

        Assert.IsTrue(Sha1Hex.TryParse(upper, out Sha1Hex a));
        Assert.IsTrue(Sha1Hex.TryParse(lower, out Sha1Hex b));

        Assert.IsTrue(a.Equals(b), "摘要比较必须与大小写无关");
        Assert.AreEqual(lower, a.Value, "归一化为小写");

        Assert.IsFalse(Sha1Hex.TryParse(null, out _));
        Assert.IsFalse(Sha1Hex.TryParse(string.Empty, out _));
        Assert.IsFalse(Sha1Hex.TryParse("abc", out _), "长度不足必须拒绝");
        Assert.IsFalse(Sha1Hex.TryParse(new string('g', 40), out _), "非十六进制字符必须拒绝");
    }
}
