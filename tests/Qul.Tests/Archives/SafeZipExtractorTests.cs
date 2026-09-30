using System;
using System.IO;
using System.IO.Compression;
using System.Text;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Domain.Diagnostics;
using Qul.Domain.Security;
using Qul.Infrastructure.Archives;

namespace Qul.Tests.Archives;

/// <summary>
/// 解压安全的端到端测试。策略是**整包拒绝**：
/// 只要有一条越界，一个字节都不许落盘——先解压再补救是做不到的，恶意条目一旦落盘就已经生效。
/// </summary>
[TestClass]
public sealed class SafeZipExtractorTests
{
    private string? _sandbox;

    [TestCleanup]
    public void Cleanup()
    {
        if (_sandbox != null && Directory.Exists(_sandbox))
        {
            try
            {
                Directory.Delete(_sandbox, recursive: true);
            }
            catch (IOException)
            {
            }
        }
    }

    [TestMethod]
    public void ExtractsNormalArchive()
    {
        string sandbox = NewSandbox();
        string target = Path.Combine(sandbox, "out");
        string zip = CreateZip(
            Path.Combine(sandbox, "ok.zip"),
            new Entry("a.txt", "alpha"),
            new Entry("dir/b.txt", "beta"),
            new Entry("dir/deep/c.txt", "gamma"));

        ExtractionReport report = SafeZipExtractor.Extract(zip, target);

        Assert.IsTrue(report.Succeeded, report.Error?.ToString());
        Assert.AreEqual(3, report.ExtractedFiles);
        Assert.AreEqual("alpha", File.ReadAllText(Path.Combine(target, "a.txt")));
        Assert.AreEqual("gamma", File.ReadAllText(Path.Combine(target, @"dir\deep\c.txt")));
    }

    [TestMethod]
    public void RejectsArchiveWithParentTraversalAndWritesNothing()
    {
        string sandbox = NewSandbox();
        string target = Path.Combine(sandbox, "out");
        string zip = CreateZip(
            Path.Combine(sandbox, "traversal.zip"),
            new Entry("harmless.txt", "ok"),
            new Entry("../escaped.txt", "pwned"));

        ExtractionReport report = SafeZipExtractor.Extract(zip, target);

        Assert.AreEqual(ErrorCode.ZipEntryEscape, report.Error);
        Assert.AreEqual(ArchivePathVerdict.ParentTraversal, report.FirstVerdict);
        Assert.AreEqual(0, report.ExtractedFiles, "越界条目必须导致整包拒绝，连正常条目也不写");
        Assert.IsFalse(File.Exists(Path.Combine(sandbox, "escaped.txt")), "越界文件绝不能落到根目录之外");
        Assert.IsFalse(Directory.Exists(target) && Directory.GetFiles(target, "*", SearchOption.AllDirectories).Length > 0);
    }

    [TestMethod]
    public void RejectsArchiveWithAbsolutePathEntry()
    {
        string sandbox = NewSandbox();
        string target = Path.Combine(sandbox, "out");
        string zip = CreateZip(
            Path.Combine(sandbox, "absolute.zip"),
            new Entry("/absolute.txt", "pwned"));

        ExtractionReport report = SafeZipExtractor.Extract(zip, target);

        Assert.AreEqual(ErrorCode.ZipEntryEscape, report.Error);
        Assert.AreEqual(ArchivePathVerdict.AbsolutePath, report.FirstVerdict);
        Assert.AreEqual(0, report.ExtractedFiles);
    }

    [TestMethod]
    public void RejectsArchiveWithDriveQualifiedEntry()
    {
        string sandbox = NewSandbox();
        string target = Path.Combine(sandbox, "out");
        string zip = CreateZip(
            Path.Combine(sandbox, "drive.zip"),
            new Entry(@"C:\Windows\Temp\pwned.txt", "pwned"));

        ExtractionReport report = SafeZipExtractor.Extract(zip, target);

        Assert.AreEqual(ErrorCode.ZipEntryEscape, report.Error);
        Assert.AreEqual(ArchivePathVerdict.DriveLetter, report.FirstVerdict);
    }

    [TestMethod]
    public void RejectsSymbolicLinkEntry()
    {
        string sandbox = NewSandbox();
        string target = Path.Combine(sandbox, "out");
        string zip = Path.Combine(sandbox, "symlink.zip");

        using (FileStream stream = new FileStream(zip, FileMode.Create, FileAccess.Write))
        using (ZipArchive archive = new ZipArchive(stream, ZipArchiveMode.Create))
        {
            ZipArchiveEntry entry = archive.CreateEntry("link.txt");
            // S_IFLNK | 0777 放在 Unix 模式位（高 16 位）上。
            entry.ExternalAttributes = unchecked((int)0xA1FF0000u);
            using (Stream body = entry.Open())
            {
                body.WriteByte((byte)'x');
            }
        }

        ExtractionReport report = SafeZipExtractor.Extract(zip, target);

        Assert.AreEqual(ErrorCode.ZipEntryEscape, report.Error);
        Assert.AreEqual(ArchivePathVerdict.SymbolicLink, report.FirstVerdict);
    }

    [TestMethod]
    public void HonoursExcludePrefixes()
    {
        string sandbox = NewSandbox();
        string target = Path.Combine(sandbox, "out");
        string zip = CreateZip(
            Path.Combine(sandbox, "natives.zip"),
            new Entry("META-INF/MANIFEST.MF", "manifest"),
            new Entry("META-INF/services/x", "service"),
            new Entry("org/lwjgl/lwjgl.dll", "binary"));

        ExtractionReport report = SafeZipExtractor.Extract(zip, target, new[] { "META-INF/" });

        Assert.IsTrue(report.Succeeded, report.Error?.ToString());
        Assert.AreEqual(1, report.ExtractedFiles);
        Assert.IsTrue(File.Exists(Path.Combine(target, @"org\lwjgl\lwjgl.dll")));
        Assert.IsFalse(Directory.Exists(Path.Combine(target, "META-INF")), "排除前缀下的内容不得落盘");

        // 写成不带斜杠的形式也要能排除。
        string target2 = Path.Combine(sandbox, "out2");
        ExtractionReport report2 = SafeZipExtractor.Extract(zip, target2, new[] { "META-INF" });
        Assert.AreEqual(1, report2.ExtractedFiles);
    }

    [TestMethod]
    public void ReportsInvalidArchive()
    {
        string sandbox = NewSandbox();
        string zip = Path.Combine(sandbox, "broken.zip");
        File.WriteAllBytes(zip, Encoding.UTF8.GetBytes("this is definitely not a zip archive"));

        ExtractionReport report = SafeZipExtractor.Extract(zip, Path.Combine(sandbox, "out"));

        Assert.AreEqual(ErrorCode.ZipInvalid, report.Error);
        Assert.AreEqual(0, report.ExtractedFiles);
    }

    [TestMethod]
    public void ReportsMissingArchiveAsNotWritable()
    {
        string sandbox = NewSandbox();

        ExtractionReport report = SafeZipExtractor.Extract(
            Path.Combine(sandbox, "does-not-exist.zip"),
            Path.Combine(sandbox, "out"));

        Assert.IsNotNull(report.Error);
        Assert.AreEqual(0, report.ExtractedFiles);
    }

    // ---------- 工具 ----------

    private string NewSandbox()
    {
        _sandbox = Path.Combine(Path.GetTempPath(), "qul-zip", Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_sandbox);
        return _sandbox;
    }

    private readonly struct Entry
    {
        public Entry(string name, string content)
        {
            Name = name;
            Content = content;
        }

        public string Name { get; }

        public string Content { get; }
    }

    private static string CreateZip(string path, params Entry[] entries)
    {
        using (FileStream stream = new FileStream(path, FileMode.Create, FileAccess.Write))
        using (ZipArchive archive = new ZipArchive(stream, ZipArchiveMode.Create))
        {
            foreach (Entry spec in entries)
            {
                ZipArchiveEntry entry = archive.CreateEntry(spec.Name);
                using (Stream body = entry.Open())
                using (StreamWriter writer = new StreamWriter(body, new UTF8Encoding(false)))
                {
                    writer.Write(spec.Content);
                }
            }
        }

        return path;
    }
}
