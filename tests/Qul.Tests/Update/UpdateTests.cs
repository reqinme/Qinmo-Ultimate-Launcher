using System;
using System.IO;
using System.Text;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Application.Update;
using Qul.Domain.Diagnostics;
using Qul.Infrastructure.Update;

namespace Qul.Tests.Update;

/// <summary>
/// P7 更新机制：版本比较、自替换、回退判定、路径白名单。
///
/// 自替换被刻意做成纯文件路径操作，所以**每个分支都能用临时文件验到**，
/// 唯一不可测的部分被压缩到两个 File.Move。
/// </summary>
[TestClass]
public sealed class UpdateTests
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

    // ================= 版本比较 =================

    [TestMethod]
    public void Version_ComparesNumericallyNotLexically()
    {
        // 字符串比较会把 "1.10.0" 判成小于 "1.9.0"——这类 bug 只在真实升级时才暴露。
        Assert.IsTrue(LauncherVersion.IsNewer("1.10.0", "1.9.0"));
        Assert.IsTrue(LauncherVersion.IsNewer("2.0.0", "1.99.99"));
        Assert.IsTrue(LauncherVersion.IsNewer("1.0.1", "1.0.0"));
        Assert.IsFalse(LauncherVersion.IsNewer("1.0.0", "1.0.0"));
        Assert.IsFalse(LauncherVersion.IsNewer("0.9.9", "1.0.0"));
    }

    [TestMethod]
    public void Version_TreatsPrereleaseAsOlderThanRelease()
    {
        Assert.IsTrue(LauncherVersion.IsNewer("1.0.0", "1.0.0-rc1"));
        Assert.IsFalse(LauncherVersion.IsNewer("1.0.0-rc1", "1.0.0"));
        Assert.IsFalse(LauncherVersion.IsNewer("1.0.0-rc1", "1.0.0-rc2"), "同为预发布版时按主号判断，不比较后缀");
    }

    [TestMethod]
    public void Version_SurvivesGarbageInput()
    {
        Assert.AreEqual(0, LauncherVersion.Compare(null, string.Empty));
        Assert.AreEqual(0, LauncherVersion.Compare("abc", "0.0.0"));
        Assert.IsTrue(LauncherVersion.IsNewer("1.0", "0.9"));
    }

    // ================= 自替换 =================

    [TestMethod]
    public void Apply_SwapsTheFileAndKeepsABackup()
    {
        string self = NewFile("app.exe", "OLD");
        string next = NewFile("pending.exe", "NEW");
        string backup = Path.Combine(_sandbox!, "app.exe.old");

        SelfReplaceOutcome outcome = SelfReplacer.Apply(self, next, backup);

        Assert.IsTrue(outcome.Succeeded, outcome.FailureReason);
        Assert.AreEqual("NEW", File.ReadAllText(self), "原路径上应当是新的内容");
        Assert.AreEqual("OLD", File.ReadAllText(backup), "旧版本必须留在备份里，否则无从回退");
        Assert.IsFalse(File.Exists(next), "待应用文件应当已被移走");
    }

    [TestMethod]
    public void Apply_RefusesWhenTheNewFileIsMissing()
    {
        string self = NewFile("app.exe", "OLD");

        SelfReplaceOutcome outcome = SelfReplacer.Apply(self, Path.Combine(_sandbox!, "nope.exe"), Path.Combine(_sandbox!, "b.old"));

        Assert.IsFalse(outcome.Succeeded);
        Assert.AreEqual("OLD", File.ReadAllText(self), "失败不得动到原文件");
    }

    [TestMethod]
    public void Apply_RestoresTheOriginalWhenTheSecondMoveFails()
    {
        // 让两步都指向同一个文件：第一步把它改名走了，第二步自然找不到源。
        // 这是真实可能发生的"同路径"错误，也正好是"第二步失败"这条分支。
        string self = NewFile("app.exe", "OLD");
        string backup = Path.Combine(_sandbox!, "app.exe.old");

        SelfReplaceOutcome outcome = SelfReplacer.Apply(self, self, backup);

        Assert.IsFalse(outcome.Succeeded);
        Assert.IsTrue(outcome.RolledBack, "进程还活着，就该当场把原文件改回来");
        Assert.IsTrue(File.Exists(self), "绝不能让用户留下一个打不开的快捷方式");
        Assert.AreEqual("OLD", File.ReadAllText(self));
    }

    [TestMethod]
    public void Rollback_PutsTheBackupBackAndDiscardsTheBrokenOne()
    {
        string self = NewFile("app.exe", "BROKEN");
        string backup = NewFile("app.exe.old", "GOOD");
        string discard = Path.Combine(_sandbox!, "app.exe.failed");

        SelfReplaceOutcome outcome = SelfReplacer.Rollback(self, backup, discard);

        Assert.IsTrue(outcome.Succeeded, outcome.FailureReason);
        Assert.AreEqual("GOOD", File.ReadAllText(self));
        Assert.AreEqual("BROKEN", File.ReadAllText(discard), "坏掉的那份挪到一边，不删，便于排查");
        Assert.IsFalse(File.Exists(backup));
    }

    [TestMethod]
    public void Rollback_FailsCleanlyWithoutABackup()
    {
        string self = NewFile("app.exe", "BROKEN");

        SelfReplaceOutcome outcome = SelfReplacer.Rollback(self, Path.Combine(_sandbox!, "none.old"), Path.Combine(_sandbox!, "d.failed"));

        Assert.IsFalse(outcome.Succeeded);
        Assert.AreEqual("BROKEN", File.ReadAllText(self));
    }

    // ================= 回退判定 =================

    [TestMethod]
    public void ShouldRollback_IsTrueOnlyWhileTheMarkerSaysSwapping()
    {
        UpdateStore store = NewStore();

        Assert.IsFalse(store.ShouldRollback(out PendingUpdate? none), "没有标记就没什么可回退的");
        Assert.IsNull(none);

        store.SavePending(new PendingUpdate
        {
            TargetVersion = "1.1.0",
            PendingFileName = "pending.exe",
            BackupFileName = "QinmoUltimateLauncher.exe.old",
            Stage = PendingUpdate.StageSwapping,
            StartedAt = DateTimeOffset.UtcNow,
        });

        // 核心语义：标记还在 + 阶段仍是 swapping ⇒ 上次替换后从未成功启动过
        Assert.IsTrue(store.ShouldRollback(out PendingUpdate? pending));
        Assert.AreEqual("1.1.0", pending!.TargetVersion);
    }

    [TestMethod]
    public void Marker_SurvivesARoundTripThroughDisk()
    {
        UpdateStore store = NewStore();
        DateTimeOffset when = DateTimeOffset.UtcNow;

        store.SavePending(new PendingUpdate
        {
            TargetVersion = "2.3.4",
            PendingFileName = "p.exe",
            BackupFileName = "b.old",
            Sha256 = "abc123",
            Stage = PendingUpdate.StageApplied,
            StartedAt = when,
        });

        UpdateStore reopened = new UpdateStore(store.Directory);
        PendingUpdate? loaded = reopened.LoadPending();

        Assert.IsNotNull(loaded);
        Assert.AreEqual("2.3.4", loaded!.TargetVersion);
        Assert.AreEqual("abc123", loaded.Sha256);
        Assert.AreEqual(PendingUpdate.StageApplied, loaded.Stage);
        Assert.IsTrue(Math.Abs((loaded.StartedAt - when).TotalSeconds) < 2);
    }

    [TestMethod]
    public void ClearPending_RetiresTheMarker()
    {
        UpdateStore store = NewStore();
        store.SavePending(new PendingUpdate
        {
            TargetVersion = "1.1.0",
            BackupFileName = "b.old",
            Stage = PendingUpdate.StageSwapping,
        });

        store.ClearPending();

        Assert.IsNull(store.LoadPending());
        Assert.IsFalse(store.ShouldRollback(out PendingUpdate? _));
    }

    [TestMethod]
    public void ShouldRollback_IgnoresAMarkerWithoutABackupName()
    {
        // 没有备份名就没法回退，标记本身是坏的：清掉它，别让程序卡在"要不要回退"上。
        UpdateStore store = NewStore();
        store.SavePending(new PendingUpdate
        {
            TargetVersion = "1.1.0",
            BackupFileName = string.Empty,
            Stage = PendingUpdate.StageSwapping,
        });

        Assert.IsFalse(store.ShouldRollback(out PendingUpdate? _));
        Assert.IsNull(store.LoadPending(), "坏标记应当被清掉");
    }

    [TestMethod]
    public void ShouldRollback_IgnoresUnreadableMarker()
    {
        UpdateStore store = NewStore();
        Directory.CreateDirectory(store.Directory);
        File.WriteAllText(store.PendingFilePath, "{ this is not json");

        Assert.IsFalse(store.ShouldRollback(out PendingUpdate? _));
    }

    // ================= 路径白名单 =================

    [TestMethod]
    public void ResolveBackupPath_DropsAnyDirectoryComponent()
    {
        // 标记文件是可以被手工改的，不能让里面的内容把文件操作引到用户数据目录去。
        UpdateStore store = NewStore();
        string programDir = Path.Combine(_sandbox!, "program");

        string plain = store.ResolveBackupPath(programDir, "app.exe.old");
        Assert.AreEqual(Path.Combine(programDir, "app.exe.old"), plain);

        string traversal = store.ResolveBackupPath(programDir, @"..\..\game\saves\level.dat");
        Assert.AreEqual(Path.Combine(programDir, "level.dat"), traversal, "目录成分必须被剥掉，只留文件名");

        string nested = store.ResolveBackupPath(programDir, "sub/dir/x.bin");
        Assert.AreEqual(Path.Combine(programDir, "x.bin"), nested);
    }

    [TestMethod]
    public void ResolveBackupPath_RejectsAnEmptyName()
    {
        UpdateStore store = NewStore();

        Assert.ThrowsException<LauncherException>(() => store.ResolveBackupPath(_sandbox!, string.Empty));
        Assert.ThrowsException<LauncherException>(() => store.ResolveBackupPath(_sandbox!, "   "));
    }

    // ================= 工具 =================

    private UpdateStore NewStore()
    {
        EnsureSandbox();
        return new UpdateStore(Path.Combine(_sandbox!, "updates"));
    }

    private string NewFile(string name, string content)
    {
        EnsureSandbox();
        string path = Path.Combine(_sandbox!, name);
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, content, new UTF8Encoding(false));
        return path;
    }

    private void EnsureSandbox()
    {
        if (_sandbox == null)
        {
            _sandbox = Path.Combine(Path.GetTempPath(), "qul-update", Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(_sandbox);
        }
    }
}
