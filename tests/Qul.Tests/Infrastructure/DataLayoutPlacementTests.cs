using System;
using System.IO;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Infrastructure.IO;

namespace Qul.Tests.Infrastructure;

/// <summary>
/// 便携 / 安装两态的判定。
///
/// 这一段属于 P7 的"分发形态"交付物：同一个 exe 放在可写目录就是便携版，
/// 放在不可写目录就落用户目录。判定必须真的有效，否则会出现
/// "程序照跑、但哪都找不到数据"这种最难查的状态。
/// </summary>
[TestClass]
public sealed class DataLayoutPlacementTests
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
    public void Resolve_PrefersPortableWhenTheDataRootCanBeCreated()
    {
        string exeDir = NewDir("app");
        string profile = NewDir("profile");

        DataLayout layout = DataLayout.Resolve(exeDir, profile);

        Assert.AreEqual(DataRootPlacement.Portable, layout.Placement);
        Assert.AreEqual(Path.Combine(exeDir, "data"), layout.DataRoot);
        // Resolve 只做决定、不留副作用；建目录是 EnsureCreated 的职责。
        Assert.IsFalse(Directory.Exists(layout.DataRoot), "解析阶段不该顺手把目录建出来");
        Assert.IsNull(layout.EnsureCreated(), "解析之后应当能把目录建出来");
        Assert.IsTrue(Directory.Exists(layout.DataRoot));
    }

    [TestMethod]
    public void Resolve_FallsBackWhenTheDataNameIsSquattedByAFile()
    {
        // 真实场景：exe 目录可写，但 data 这个名字已经被一个文件占住。
        // 按"exe 目录可不可写"判断会选便携模式，随后建目录失败——
        // 程序照跑，却一个日志都不写。所以探测的必须是便携数据根本身。
        string exeDir = NewDir("app");
        string profile = NewDir("profile");
        File.WriteAllText(Path.Combine(exeDir, "data"), "not a directory");

        DataLayout layout = DataLayout.Resolve(exeDir, profile);

        Assert.AreEqual(DataRootPlacement.UserProfile, layout.Placement);
        StringAssert.StartsWith(layout.DataRoot, profile);
        Assert.IsFalse(File.Exists(layout.DataRoot), "回退目标不该被同名文件占住");
        Assert.IsNull(layout.EnsureCreated());
        Assert.IsTrue(Directory.Exists(layout.DataRoot));
    }

    [TestMethod]
    public void Resolve_KeepsGameRootBesideTheDataRoot()
    {
        // 游戏目录与数据根同级，便于整个目录一起搬走。
        string exeDir = NewDir("app");
        string profile = NewDir("profile");

        DataLayout layout = DataLayout.Resolve(exeDir, profile);

        Assert.AreEqual(exeDir, Path.GetDirectoryName(layout.DataRoot));
        Assert.AreEqual(exeDir, Path.GetDirectoryName(layout.GameRoot));
    }

    [TestMethod]
    public void Resolve_RejectsMissingInput()
    {
        Assert.ThrowsException<ArgumentException>(() => DataLayout.Resolve(string.Empty, "x"));
        Assert.ThrowsException<ArgumentException>(() => DataLayout.Resolve("x", string.Empty));
    }

    private string NewDir(string name)
    {
        if (_sandbox == null)
        {
            _sandbox = Path.Combine(Path.GetTempPath(), "qul-layout", Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(_sandbox);
        }

        string path = Path.Combine(_sandbox, name);
        Directory.CreateDirectory(path);
        return path;
    }
}
