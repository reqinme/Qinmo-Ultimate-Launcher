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
        Assert.AreEqual(UpdateBootDecision.None, store.DecideOnBoot("1.1.0", out PendingUpdate? _));
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

    // ================= 启动决策状态机 =================

    [TestMethod]
    public void DecideOnBoot_IsQuietWithoutAMarker()
    {
        UpdateStore store = NewStore();

        Assert.AreEqual(UpdateBootDecision.None, store.DecideOnBoot("0.1.0", out PendingUpdate? pending));
        Assert.IsNull(pending);
    }

    [TestMethod]
    public void DecideOnBoot_AdoptsWhenWeAreTheVersionThatWasJustSwappedIn()
    {
        // 替换刚完成时，新版本的第一次启动**本来就会看到 swapping**。
        // 把它当成失败去回滚，等于每次更新都自杀一次。
        UpdateStore store = NewStore();
        store.SavePending(new PendingUpdate
        {
            TargetVersion = "0.2.0",
            PendingFileName = "pending.exe",
            BackupFileName = "app.exe.old",
            Stage = PendingUpdate.StageSwapping,
            StartedAt = DateTimeOffset.UtcNow,
        });

        Assert.AreEqual(UpdateBootDecision.Adopt, store.DecideOnBoot("0.2.0", out PendingUpdate? _));

        // 采纳之后阶段必须推进到 applied，否则下次启动又会走一遍 Adopt，永远确认不了健康
        PendingUpdate? after = store.LoadPending();
        Assert.IsNotNull(after);
        Assert.AreEqual(PendingUpdate.StageApplied, after!.Stage);
    }

    [TestMethod]
    public void DecideOnBoot_DiscardsWhenTheSwapNeverHappened()
    {
        // 运行的还是旧版本 ⇒ 替换没发生过，标记是残留。旧版本完好，清掉即可。
        UpdateStore store = NewStore();
        store.SavePending(new PendingUpdate
        {
            TargetVersion = "0.2.0",
            BackupFileName = "app.exe.old",
            Stage = PendingUpdate.StageSwapping,
        });

        UpdateBootDecision decision = store.DecideOnBoot("0.1.0", out PendingUpdate? _);

        Assert.AreEqual(UpdateBootDecision.Discard, decision);
        Assert.IsNotNull(store.LoadPending(), "Discard 只做判定，清理由调用方执行");
    }

    [TestMethod]
    public void DecideOnBoot_RollsBackWhenTheLastUpdateNeverConfirmedHealth()
    {
        // 核心场景：新版本启动过一次、打过 applied 标记，却从未确认健康 ⇒ 上次启动中途死了。
        UpdateStore store = NewStore();
        store.SavePending(new PendingUpdate
        {
            TargetVersion = "0.2.0",
            BackupFileName = "app.exe.old",
            Stage = PendingUpdate.StageApplied,
            StartedAt = DateTimeOffset.UtcNow,
        });

        Assert.AreEqual(UpdateBootDecision.Rollback, store.DecideOnBoot("0.2.0", out PendingUpdate? pending));
        Assert.AreEqual("0.2.0", pending!.TargetVersion);
    }

    [TestMethod]
    public void DecideOnBoot_DiscardsAnUnknownStage()
    {
        UpdateStore store = NewStore();
        store.SavePending(new PendingUpdate
        {
            TargetVersion = "0.2.0",
            BackupFileName = "app.exe.old",
            Stage = "something-else",
        });

        Assert.AreEqual(UpdateBootDecision.Discard, store.DecideOnBoot("0.2.0", out PendingUpdate? _));
    }

    // ================= 发布清单 =================

    private const string ManifestJson = @"{
      ""formatVersion"": 1,
      ""releases"": [
        { ""version"": ""0.2.0"", ""downloadUrl"": ""https://example.invalid/a.exe"", ""sha256"": ""aa"", ""sizeBytes"": 100 },
        { ""version"": ""0.3.0"", ""downloadUrl"": ""https://example.invalid/b.exe"", ""sha256"": ""bb"", ""minimumVersion"": ""0.2.0"" },
        { ""version"": ""0.4.0"", ""downloadUrl"": ""https://example.invalid/c.exe"" },
        { ""version"": ""0.1.0"", ""downloadUrl"": ""https://example.invalid/d.exe"", ""sha256"": ""dd"" }
      ]
    }";

    [TestMethod]
    public void Manifest_ParsesEveryField()
    {
        ReleaseManifest manifest = UpdateService.ParseManifest(ManifestJson);

        Assert.AreEqual(1, manifest.FormatVersion);
        Assert.AreEqual(4, manifest.Releases.Count);
        Assert.AreEqual("0.2.0", manifest.Releases[0].Version);
        Assert.AreEqual("https://example.invalid/a.exe", manifest.Releases[0].DownloadUrl);
        Assert.AreEqual("aa", manifest.Releases[0].Sha256);
        Assert.AreEqual(100, manifest.Releases[0].SizeBytes);
        Assert.AreEqual("0.2.0", manifest.Releases[1].MinimumVersion);
    }

    [TestMethod]
    public void Manifest_RefusesAnUnknownFormat()
    {
        Assert.ThrowsException<LauncherException>(() =>
            UpdateService.ParseManifest("{\"formatVersion\":99,\"releases\":[]}"));
    }

    [TestMethod]
    public void Pick_SkipsReleasesWithoutADigest()
    {
        // 没有摘要就不允许更新：宁可让用户手动换文件，
        // 也不能把来源不明的可执行文件放到用户机器上执行。
        ReleaseManifest manifest = UpdateService.ParseManifest(ManifestJson);

        UpdateRelease? picked = manifest.Pick("0.1.0");

        Assert.IsNotNull(picked);
        Assert.AreNotEqual("0.4.0", picked!.Version, "0.4.0 没有 sha256，必须被跳过");
    }

    [TestMethod]
    public void Pick_RespectsMinimumVersion()
    {
        // 从 0.1.0 出发时，0.3.0 要求至少 0.2.0 —— 够不着，只能升到 0.2.0。
        ReleaseManifest manifest = UpdateService.ParseManifest(ManifestJson);

        Assert.AreEqual("0.2.0", manifest.Pick("0.1.0")!.Version);
        Assert.AreEqual("0.3.0", manifest.Pick("0.2.0")!.Version);
    }

    [TestMethod]
    public void Pick_ReturnsNothingWhenAlreadyCurrent()
    {
        ReleaseManifest manifest = UpdateService.ParseManifest(ManifestJson);

        Assert.IsNull(manifest.Pick("0.3.0"), "没有比 0.3.0 更新且可用的版本");
        Assert.IsNull(manifest.Pick("9.9.9"));
    }

    [TestMethod]
    public void DecideOnBoot_MatchesVersionsAcrossDifferentSegmentCounts()
    {
        // 程序集版本是四段（0.2.0.0），发布清单里通常写三段（0.2.0）。
        // 用字符串相等判断会把它们判成不同，于是新版本启动时走 Discard：
        // 标记被清掉，**真正坏掉的更新再也不会被回滚**。这比不更新危险得多。
        UpdateStore store = NewStore();
        store.SavePending(new PendingUpdate
        {
            TargetVersion = "0.2.0",
            PendingFileName = "pending.exe",
            BackupFileName = "app.exe.old",
            Stage = PendingUpdate.StageSwapping,
            StartedAt = DateTimeOffset.UtcNow,
        });

        Assert.AreEqual(UpdateBootDecision.Adopt, store.DecideOnBoot("0.2.0.0", out PendingUpdate? _));
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
    [TestMethod]
    public void Decide_FallsBackToTheRecordedHashWhenTheVersionStringDoesNotCompareEqual()
    {
        // **这条用例守的是"自更新安全网被静默关闭"。**
        //
        // 发布清单可能写 `0.2.0-rc1`，而程序集版本是 `0.2.0.0`，
        // 版本比较不相等。只看版本号的话就会走 Discard —— 标记被清掉，
        // **真正坏掉的更新再也不会被回滚**，整套两阶段标记对这类版本形同虚设。
        //
        // 标记里记着新版本的 sha256，用它判断既精确又与版本号的写法无关。
        const string NewHash = "8f0112b87ec30c3a895feba48a309d45aec38432bce21d6633cab83b1c039003";

        PendingUpdate pending = new PendingUpdate
        {
            TargetVersion = "0.2.0-rc1",
            PendingFileName = "pending.exe",
            BackupFileName = "app.exe.old",
            Sha256 = NewHash,
            Stage = PendingUpdate.StageSwapping,
        };

        // 版本号对不上，但运行中的文件就是标记里记的那个 ⇒ 必须认领，不能丢标记
        Assert.AreEqual(
            UpdateBootDecision.Adopt,
            UpdateStore.Decide("0.2.0.0", pending, NewHash));

        // 大小写不同也要认（sha256 的十六进制大小写不敏感）
        Assert.AreEqual(
            UpdateBootDecision.Adopt,
            UpdateStore.Decide("0.2.0.0", pending, NewHash.ToUpperInvariant()));

        // 运行中的文件不是标记里那个 ⇒ 替换确实没发生过，才是 Discard
        Assert.AreEqual(
            UpdateBootDecision.Discard,
            UpdateStore.Decide("0.1.0.0", pending, "0000000000000000000000000000000000000000000000000000000000000000"));
    }

    [TestMethod]
    public void Decide_KeepsTheVersionFastPathAndTheAppliedRollbackRule()
    {
        PendingUpdate pending = new PendingUpdate
        {
            TargetVersion = "0.2.0",
            PendingFileName = "pending.exe",
            BackupFileName = "app.exe.old",
            Sha256 = "aaaa",
            Stage = PendingUpdate.StageSwapping,
        };

        // 版本号直接相等：不必去算哈希
        Assert.AreEqual(UpdateBootDecision.Adopt, UpdateStore.Decide("0.2.0", pending, null));

        // 分段数不同（0.2.0 vs 0.2.0.0）也要认——先前为这个单独修过一次
        Assert.AreEqual(UpdateBootDecision.Adopt, UpdateStore.Decide("0.2.0.0", pending, null));

        // stage=applied 且从未确认健康 ⇒ 上次启动中途死了，必须回退
        pending.Stage = PendingUpdate.StageApplied;
        Assert.AreEqual(UpdateBootDecision.Rollback, UpdateStore.Decide("0.2.0", pending, "aaaa"));

        // 没有标记就是没事
        Assert.AreEqual(UpdateBootDecision.None, UpdateStore.Decide("0.2.0", null!, null));

        // 哈希算不出来（例如受限环境）时不能因此认领，也不能崩
        PendingUpdate other = new PendingUpdate
        {
            TargetVersion = "9.9.9",
            Sha256 = "bbbb",
            Stage = PendingUpdate.StageSwapping,
        };
        Assert.AreEqual(UpdateBootDecision.Discard, UpdateStore.Decide("0.2.0", other, null));
    }
}