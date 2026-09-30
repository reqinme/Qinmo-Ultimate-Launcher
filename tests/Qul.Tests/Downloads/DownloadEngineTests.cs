using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Net;
using System.Security.Cryptography;
using System.Text;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Application.Ports;
using Qul.Domain.Diagnostics;
using Qul.Domain.Downloads;
using Qul.Infrastructure.Downloads;
using Qul.Infrastructure.Net;

namespace Qul.Tests.Downloads;

/// <summary>
/// 下载引擎的行为测试。全部走伪造传输 + 真实临时目录：
/// 网络是模拟的（否则测不出"拔网线"），磁盘是真的（否则测不出一致性）。
/// </summary>
[TestClass]
public sealed class DownloadEngineTests
{
    private const string Url = "https://example.invalid/lib.jar";

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

    // ---------- 缓存命中与校验 ----------

    [TestMethod]
    public void EnsureAll_DownloadsMissingFile_ThenSkipsItOnSecondRun()
    {
        string sandbox = NewSandbox();
        byte[] body = Bytes("library-content");
        FakeTransport transport = new FakeTransport();
        transport.Serve(Url, body);

        DownloadEngine engine = new DownloadEngine(transport);
        DownloadPlan plan = Plan(Item(Url, body, @"libraries\a\a\1.0\a-1.0.jar"));

        DownloadReport first = engine.EnsureAll(plan, sandbox, FastOptions());

        Assert.IsTrue(first.IsComplete);
        Assert.AreEqual(1, first.DownloadedCount);
        Assert.AreEqual(1, transport.Requests.Count);
        CollectionAssert.AreEqual(body, File.ReadAllBytes(Path.Combine(sandbox, @"libraries\a\a\1.0\a-1.0.jar")));

        DownloadReport second = engine.EnsureAll(plan, sandbox, FastOptions());

        Assert.IsTrue(second.IsComplete);
        Assert.AreEqual(1, second.PresentCount);
        Assert.AreEqual(0, second.DownloadedCount);
        Assert.AreEqual(1, transport.Requests.Count, "缓存命中时不得产生任何网络请求");
        Assert.AreEqual(0, second.BytesTransferred);
    }

    [TestMethod]
    public void EnsureAll_RedownloadsWhenCachedFileIsCorrupt()
    {
        string sandbox = NewSandbox();
        byte[] body = Bytes("the-real-content");
        string destination = Path.Combine(sandbox, @"libraries\c.jar");
        Directory.CreateDirectory(Path.GetDirectoryName(destination)!);
        File.WriteAllBytes(destination, Bytes("corrupted-on-disk"));

        FakeTransport transport = new FakeTransport();
        transport.Serve(Url, body);

        DownloadReport report = new DownloadEngine(transport)
            .EnsureAll(Plan(Item(Url, body, @"libraries\c.jar")), sandbox, FastOptions());

        Assert.IsTrue(report.IsComplete);
        Assert.AreEqual(1, report.DownloadedCount);
        Assert.AreEqual(1, transport.Requests.Count);
        CollectionAssert.AreEqual(body, File.ReadAllBytes(destination), "损坏的缓存必须被检出并重下");
    }

    [TestMethod]
    public void EnsureAll_RefusesItemWithoutChecksum()
    {
        string sandbox = NewSandbox();
        FakeTransport transport = new FakeTransport();

        DownloadItem item = new DownloadItem
        {
            Kind = DownloadItemKind.Library,
            Url = Url,
            Sha1 = null,
            RelativePath = @"libraries\nope.jar",
        };

        DownloadReport report = new DownloadEngine(transport).EnsureAll(Plan(item), sandbox, FastOptions());

        Assert.AreEqual(1, report.Failures.Count);
        Assert.AreEqual(DownloadItemState.RefusedNoChecksum, report.Items[0].State);
        Assert.AreEqual(ErrorCode.DlChecksumMissing, report.Items[0].Error);
        Assert.AreEqual(0, transport.Requests.Count, "没有校验基准时连请求都不该发出去");
        Assert.IsFalse(File.Exists(Path.Combine(sandbox, @"libraries\nope.jar")));
    }

    [TestMethod]
    public void EnsureAll_FailsAfterAttemptsWhenChecksumNeverMatches()
    {
        string sandbox = NewSandbox();
        byte[] expected = Bytes("expected-content");
        byte[] served = Bytes("tampered-content");

        FakeTransport transport = new FakeTransport();
        transport.Serve(Url, served);

        DownloadOptions options = FastOptions();
        options.MaxAttempts = 3;

        DownloadReport report = new DownloadEngine(transport)
            .EnsureAll(Plan(Item(Url, expected, @"libraries\t.jar")), sandbox, options);

        Assert.AreEqual(1, report.Failures.Count);
        Assert.AreEqual(ErrorCode.DlChecksumMismatch, report.Items[0].Error);
        Assert.AreEqual(3, report.Items[0].Attempts);
        Assert.AreEqual(3, transport.Requests.Count);
        Assert.IsFalse(File.Exists(Path.Combine(sandbox, @"libraries\t.jar")), "校验不通过的成品绝不能留下");
        Assert.IsFalse(File.Exists(Path.Combine(sandbox, @"libraries\t.jar.part")), "校验不通过的残留也绝不能留下");
    }

    // ---------- 续传 ----------

    [TestMethod]
    public void EnsureAll_ResumesFromTheBreakPointInsteadOfStartingOver()
    {
        string sandbox = NewSandbox();
        byte[] body = Bytes("0123456789ABCDEFGHIJ");
        const int BreakAt = 8;

        FakeTransport transport = new FakeTransport();
        transport.Serve(Url, body);

        // 第一次调用吐 8 个字节后掐断，之后恢复正常。
        transport.Interceptor = (request, call) => call == 1 ? FakeTransport.Truncated(body, BreakAt) : null;

        DownloadReport report = new DownloadEngine(transport)
            .EnsureAll(Plan(Item(Url, body, @"libraries\r.jar")), sandbox, FastOptions());

        Assert.IsTrue(report.IsComplete, "断线后必须能自行收敛");
        Assert.AreEqual(1, report.DownloadedCount);
        Assert.AreEqual(2, report.Items[0].Attempts);
        Assert.AreEqual(2, transport.Requests.Count);

        Assert.IsNull(transport.Requests[0].RangeFrom, "首次请求不带 Range");
        Assert.AreEqual(
            (long)BreakAt,
            transport.Requests[1].RangeFrom,
            "重试必须从断点续传，而不是从头再来");

        CollectionAssert.AreEqual(
            body,
            File.ReadAllBytes(Path.Combine(sandbox, @"libraries\r.jar")),
            "续传拼接后的内容必须与源完全一致");
    }

    [TestMethod]
    public void EnsureAll_DiscardsPartialWhenServerRejectsTheRange()
    {
        string sandbox = NewSandbox();
        byte[] body = Bytes("hello world");
        string partial = Path.Combine(sandbox, @"libraries\p.jar.part");
        Directory.CreateDirectory(Path.GetDirectoryName(partial)!);
        File.WriteAllBytes(partial, new byte[body.Length + 10]); // 本地残留比远端还长

        FakeTransport transport = new FakeTransport();
        transport.Serve(Url, body);
        transport.Interceptor = (request, call) => call == 1
            ? new HttpFetchResponse { Status = HttpFetchStatus.RangeNotSatisfiable, StatusCode = 416, Content = Stream.Null }
            : null;

        DownloadReport report = new DownloadEngine(transport)
            .EnsureAll(Plan(Item(Url, body, @"libraries\p.jar")), sandbox, FastOptions());

        Assert.IsTrue(report.IsComplete, "残留不可用时必须丢弃残留并从头重来");
        Assert.AreEqual(2, transport.Requests.Count);
        Assert.AreEqual(
            (long)(body.Length + 10),
            transport.Requests[0].RangeFrom,
            "第一次应当带着残留长度去要 Range");
        Assert.IsNull(transport.Requests[1].RangeFrom, "残留被丢弃后第二次必须从头取");
        CollectionAssert.AreEqual(body, File.ReadAllBytes(Path.Combine(sandbox, @"libraries\p.jar")));
    }

    [TestMethod]
    public void EnsureAll_AppendsOnlyWhenServerReallyHonoursTheRange()
    {
        string sandbox = NewSandbox();
        byte[] body = Bytes("0123456789");
        string partial = Path.Combine(sandbox, @"libraries\h.jar.part");
        Directory.CreateDirectory(Path.GetDirectoryName(partial)!);
        File.WriteAllBytes(partial, Bytes("01234"));

        FakeTransport transport = new FakeTransport();
        transport.Serve(Url, body);

        // 服务端忽略 Range，回 200 带全部内容——绝不能追加，必须覆盖重写。
        transport.Interceptor = (request, call) => request.RangeFrom.HasValue
            ? new HttpFetchResponse
            {
                Status = HttpFetchStatus.Success,
                StatusCode = 200,
                ContentLength = body.Length,
                TotalLength = body.Length,
                RangeStart = 0,
                Content = new MemoryStream(body),
            }
            : null;

        DownloadReport report = new DownloadEngine(transport)
            .EnsureAll(Plan(Item(Url, body, @"libraries\h.jar")), sandbox, FastOptions());

        Assert.IsTrue(report.IsComplete);
        CollectionAssert.AreEqual(
            body,
            File.ReadAllBytes(Path.Combine(sandbox, @"libraries\h.jar")),
            "服务端忽略 Range 时必须整体重写，追加会得到重复内容");
    }

    // ---------- 重试与错误分类 ----------

    [TestMethod]
    public void EnsureAll_RetriesOnRetryableNetworkFailure()
    {
        string sandbox = NewSandbox();
        byte[] body = Bytes("retry-me");

        FakeTransport transport = new FakeTransport();
        transport.Serve(Url, body);
        transport.Interceptor = (request, call) => call == 1
            ? throw new LauncherException(ErrorCode.NetUnreachable, "simulated drop")
            : null;

        DownloadReport report = new DownloadEngine(transport)
            .EnsureAll(Plan(Item(Url, body, @"libraries\retry.jar")), sandbox, FastOptions());

        Assert.IsTrue(report.IsComplete);
        Assert.AreEqual(2, report.Items[0].Attempts);
    }

    [TestMethod]
    public void EnsureAll_DoesNotRetryWhenResourceIsMissing()
    {
        string sandbox = NewSandbox();
        byte[] body = Bytes("gone");

        FakeTransport transport = new FakeTransport();
        transport.Interceptor = (request, call) =>
            new HttpFetchResponse { Status = HttpFetchStatus.NotFound, StatusCode = 404, Content = Stream.Null };

        DownloadReport report = new DownloadEngine(transport)
            .EnsureAll(Plan(Item(Url, body, @"libraries\gone.jar")), sandbox, FastOptions());

        Assert.AreEqual(1, report.Failures.Count);
        Assert.AreEqual(ErrorCode.NetResourceMissing, report.Items[0].Error);
        Assert.AreEqual(1, report.Items[0].Attempts);
        Assert.AreEqual(1, transport.Requests.Count, "404 重试三次是纯浪费：必须一次就定性");
    }

    [TestMethod]
    public void EnsureAll_DoesNotRetryOnCertificateFailure()
    {
        string sandbox = NewSandbox();
        byte[] body = Bytes("tls");

        FakeTransport transport = new FakeTransport();
        transport.Interceptor = (request, call) =>
            throw new LauncherException(ErrorCode.NetCertificateInvalid, "simulated trust failure");

        DownloadReport report = new DownloadEngine(transport)
            .EnsureAll(Plan(Item(Url, body, @"libraries\tls.jar")), sandbox, FastOptions());

        Assert.AreEqual(ErrorCode.NetCertificateInvalid, report.Items[0].Error);
        Assert.AreEqual(1, transport.Requests.Count, "证书不通过重试没有意义");
    }

    [TestMethod]
    public void EnsureAll_IsolatesASingleItemFailure()
    {
        string sandbox = NewSandbox();
        byte[] good1 = Bytes("good-one");
        byte[] good2 = Bytes("good-two");
        byte[] bad = Bytes("bad");

        FakeTransport transport = new FakeTransport();
        transport.Serve("https://example.invalid/one.jar", good1);
        transport.Serve("https://example.invalid/two.jar", good2);
        transport.Serve("https://example.invalid/bad.jar", bad);
        transport.Interceptor = (request, call) => request.Url.EndsWith("bad.jar", StringComparison.Ordinal)
            ? throw new LauncherException(ErrorCode.NetUnreachable, "simulated permanent drop")
            : null;

        DownloadPlan plan = Plan(
            Item("https://example.invalid/one.jar", good1, @"libraries\one.jar"),
            Item("https://example.invalid/bad.jar", bad, @"libraries\bad.jar"),
            Item("https://example.invalid/two.jar", good2, @"libraries\two.jar"));

        DownloadReport report = new DownloadEngine(transport).EnsureAll(plan, sandbox, FastOptions());

        Assert.AreEqual(2, report.DownloadedCount, "一个坏条目不得牵连其他条目");
        Assert.AreEqual(1, report.Failures.Count);
        Assert.IsFalse(report.IsComplete);
        Assert.IsTrue(File.Exists(Path.Combine(sandbox, @"libraries\one.jar")));
        Assert.IsTrue(File.Exists(Path.Combine(sandbox, @"libraries\two.jar")));
    }

    // ---------- 预检与网络环境 ----------

    [TestMethod]
    public void EnsureAll_ReportsDiskFullBeforeIssuingAnyRequest()
    {
        string sandbox = NewSandbox();
        byte[] body = Bytes("needs-space");

        FakeTransport transport = new FakeTransport();
        transport.Serve(Url, body);

        DownloadOptions options = FastOptions();
        options.MinimumFreeBytes = long.MaxValue / 2;

        DownloadReport report = new DownloadEngine(transport)
            .EnsureAll(Plan(Item(Url, body, @"libraries\big.jar")), sandbox, options);

        Assert.AreEqual(ErrorCode.IoDiskFull, report.Items[0].Error);
        Assert.AreEqual(0, transport.Requests.Count, "空间不足时不该先下一半再说");
    }

    [TestMethod]
    public void EnsureAll_ExpressesSystemProxyDirectAndExplicitProxyDifferently()
    {
        string sandbox = NewSandbox();
        byte[] body = Bytes("proxy");
        string destination = Path.Combine(sandbox, @"libraries\proxy.jar");

        FakeTransport transport = new FakeTransport();
        transport.Serve(Url, body);
        DownloadEngine engine = new DownloadEngine(transport);
        DownloadPlan plan = Plan(Item(Url, body, @"libraries\proxy.jar"));

        engine.EnsureAll(plan, sandbox, new DownloadOptions { UseSystemProxy = true, BaseRetryDelay = TimeSpan.Zero });
        Assert.IsNull(transport.Requests[0].ProxyAddress, "跟随系统代理时不得覆盖代理设置");

        File.Delete(destination);
        engine.EnsureAll(plan, sandbox, new DownloadOptions { UseSystemProxy = false, ProxyAddress = null, BaseRetryDelay = TimeSpan.Zero });
        Assert.AreEqual(string.Empty, transport.Requests[1].ProxyAddress, "直连必须与跟随系统区分开");

        File.Delete(destination);
        engine.EnsureAll(plan, sandbox, new DownloadOptions { UseSystemProxy = false, ProxyAddress = "http://127.0.0.1:8080", BaseRetryDelay = TimeSpan.Zero });
        Assert.AreEqual("http://127.0.0.1:8080", transport.Requests[2].ProxyAddress);
    }

    [TestMethod]
    public void EnsureAll_ReportsProgressForEveryItem()
    {
        string sandbox = NewSandbox();
        byte[] a = Bytes("aaa");
        byte[] b = Bytes("bbb");

        FakeTransport transport = new FakeTransport();
        transport.Serve("https://example.invalid/a.jar", a);
        transport.Serve("https://example.invalid/b.jar", b);

        RecordingProgress progress = new RecordingProgress();

        DownloadPlan plan = Plan(
            Item("https://example.invalid/a.jar", a, @"libraries\a.jar"),
            Item("https://example.invalid/b.jar", b, @"libraries\b.jar"));

        DownloadEngine engine = new DownloadEngine(transport);
        DownloadReport report = engine.EnsureAll(plan, sandbox, FastOptions(), progress);

        Assert.IsTrue(report.IsComplete);
        Assert.AreEqual(2, report.DownloadedCount);
        Assert.AreEqual(2, progress.Updates.Count, "每个条目都应有且只有一次进度回调");
        Assert.AreEqual(2, progress.Updates.Max(u => u.FilesCompleted));
        Assert.AreEqual(2, progress.Updates.Max(u => u.FilesTotal));
    }

    // ---------- 安全基线 ----------

    [TestMethod]
    public void HttpTransport_DoesNotInstallAnyCertificateBypass()
    {
        new HttpTransport();

        Assert.IsNull(
            ServicePointManager.ServerCertificateValidationCallback,
            "绝不安装证书校验回调：证书异常必须分类上报（QUL-NET-0003），而不是被忽略掉继续走");
    }

    // ---------- 工具 ----------

    private string NewSandbox()
    {
        _sandbox = Path.Combine(Path.GetTempPath(), "qul-p2", Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_sandbox);
        return _sandbox;
    }

    private static DownloadOptions FastOptions()
    {
        return new DownloadOptions
        {
            MaxAttempts = 3,
            BaseRetryDelay = TimeSpan.Zero,
            MaxConcurrency = 2,
            MinimumFreeBytes = 0,
        };
    }

    private static DownloadPlan Plan(params DownloadItem[] items)
    {
        return new DownloadPlan { Name = "test-plan", Items = items };
    }

    private static DownloadItem Item(string url, byte[] content, string relativePath, bool optional = false)
    {
        return new DownloadItem
        {
            Kind = DownloadItemKind.Library,
            Url = url,
            Sha1 = Sha1Of(content),
            Size = content.Length,
            RelativePath = relativePath,
            IsOptional = optional,
        };
    }

    private static byte[] Bytes(string text)
    {
        return Encoding.UTF8.GetBytes(text);
    }

    private static string Sha1Of(byte[] data)
    {
        using (SHA1 sha1 = SHA1.Create())
        {
            byte[] hash = sha1.ComputeHash(data);
            StringBuilder sb = new StringBuilder(hash.Length * 2);
            foreach (byte b in hash)
            {
                sb.Append(b.ToString("x2"));
            }

            return sb.ToString();
        }
    }
    // ---------- 编排层的健壮性 ----------

    /// <summary>进度接收方在报告时抛异常。真实场景：界面已关闭、输出流已释放、日志正在轮转。</summary>
    private sealed class ThrowingProgress : System.IProgress<DownloadProgress>
    {
        public void Report(DownloadProgress value)
        {
            throw new ObjectDisposedException("progress sink");
        }
    }

    [TestMethod]
    public void EnsureAll_SurvivesAProgressSinkThatThrows()
    {
        // 进度上报失败绝不能拖垮一个已经下好的文件。
        // 先前这里没有任何保护，一个 ObjectDisposedException 就是这么逃到编排层的。
        string sandbox = NewSandbox();
        byte[] body = Bytes("library-content");
        FakeTransport transport = new FakeTransport();
        transport.Serve(Url, body);

        DownloadEngine engine = new DownloadEngine(transport);
        DownloadPlan plan = Plan(Item(Url, body, @"libraries\a\a\1.0\a-1.0.jar"));

        DownloadReport report = engine.EnsureAll(
            plan, sandbox, FastOptions(), new ThrowingProgress(), System.Threading.CancellationToken.None);

        Assert.IsTrue(report.IsComplete, "进度接收方抛异常不该影响下载结果");
        Assert.AreEqual(1, report.DownloadedCount);
        CollectionAssert.AreEqual(body, File.ReadAllBytes(Path.Combine(sandbox, @"libraries\a\a\1.0\a-1.0.jar")));
    }

    [TestMethod]
    public void EnsureAll_RetriesWhenTheConnectionIsReleasedEarly()
    {
        // 底层连接被提前释放属于可重试的传输故障。
        // 它必须留在重试循环里——逃到编排层就没有重试，条目会以"尝试 0 次"永久失败。
        string sandbox = NewSandbox();
        byte[] body = Bytes("library-content");
        FakeTransport transport = new FakeTransport();
        transport.Serve(Url, body);

        int calls = 0;
        transport.Interceptor = (request, call) =>
        {
            calls++;
            if (calls == 1)
            {
                throw new ObjectDisposedException("connection");
            }

            return null;
        };

        DownloadEngine engine = new DownloadEngine(transport);
        DownloadPlan plan = Plan(Item(Url, body, @"libraries\a\a\1.0\a-1.0.jar"));

        DownloadReport report = engine.EnsureAll(plan, sandbox, FastOptions());

        Assert.IsTrue(report.IsComplete, "第一次连接被提前释放后应当重试成功");
        Assert.AreEqual(1, report.DownloadedCount);
        Assert.AreEqual(2, report.Items[0].Attempts, "尝试次数必须如实反映重试，而不是 0");
    }

    [TestMethod]
    public void EnsureAll_ReportsRealAttemptCountWhenEveryAttemptIsReleasedEarly()
    {
        string sandbox = NewSandbox();
        byte[] body = Bytes("library-content");
        FakeTransport transport = new FakeTransport();
        transport.Serve(Url, body);
        transport.Interceptor = (request, call) => throw new ObjectDisposedException("connection");

        DownloadEngine engine = new DownloadEngine(transport);
        DownloadPlan plan = Plan(Item(Url, body, @"libraries\a\a\1.0\a-1.0.jar"));

        DownloadReport report = engine.EnsureAll(plan, sandbox, FastOptions());

        Assert.IsFalse(report.IsComplete);
        Assert.AreEqual(1, report.Failures.Count);

        // 关键断言：不是 0。0 意味着"一次都没试过"，那是编排层兜底 catch 的痕迹，
        // 也正是 26.3 那次下载里最可疑的数字。
        Assert.IsTrue(report.Failures[0].Attempts >= 1, "必须真的尝试过，而不是一次都没试就记成失败");
    }
    // ---------- 按实测速度选源 ----------

    private const string MirrorUrl = "https://bmclapi2.bangbang93.com/libraries/a/a/1.0/a-1.0.jar";

    [TestMethod]
    public void PickSample_NeedsAtLeastTwoSources()
    {
        byte[] body = Bytes("x");
        DownloadPlan single = Plan(Item(Url, body, @"libraries\a\a\1.0\a-1.0.jar"));

        Assert.IsNull(DownloadSourceProbe.PickSample(single), "只有一个源就没有可探测的东西");

        DownloadItem multi = Item(Url, body, @"libraries\a\a\1.0\a-1.0.jar");
        multi.FallbackUrls = new[] { MirrorUrl };

        Assert.IsNotNull(DownloadSourceProbe.PickSample(Plan(multi)));
    }

    [TestMethod]
    public void EnsureAll_PrefersTheFasterSourceWhenThePrimaryIsSlow()
    {
        // 核心场景：官方**慢但能通**。
        // 只按失败换源的实现永远不会切换，几千个文件就那么磨完；
        // 而"官方慢"恰恰是引入镜像的初衷。
        string sandbox = NewSandbox();
        byte[] body = Bytes("library-content");

        FakeTransport transport = new FakeTransport();
        transport.Serve(Url, body);
        transport.Serve(MirrorUrl, body);
        transport.DelayMilliseconds = url => string.Equals(url, Url, StringComparison.Ordinal) ? 120 : 0;

        DownloadEngine engine = new DownloadEngine(transport);

        DownloadItem item = Item(Url, body, @"libraries\a\a\1.0\a-1.0.jar");
        item.FallbackUrls = new[] { MirrorUrl };

        DownloadReport report = engine.EnsureAll(Plan(item), sandbox, FastOptions());

        Assert.IsTrue(report.IsComplete);

        int primary = transport.Requests.Count(r => string.Equals(r.Url, Url, StringComparison.Ordinal));
        int mirror = transport.Requests.Count(r => string.Equals(r.Url, MirrorUrl, StringComparison.Ordinal));

        Assert.AreEqual(1, primary, "官方只应被探测取一次，之后的真实下载不该再走它");
        Assert.AreEqual(2, mirror, "镜像应被取两次：探测一次 + 真实下载一次");
    }

    [TestMethod]
    public void EnsureAll_KeepsTheConfiguredOrderWhenProbingIsOff()
    {
        string sandbox = NewSandbox();
        byte[] body = Bytes("library-content");

        FakeTransport transport = new FakeTransport();
        transport.Serve(Url, body);
        transport.Serve(MirrorUrl, body);
        transport.DelayMilliseconds = url => string.Equals(url, Url, StringComparison.Ordinal) ? 120 : 0;

        DownloadEngine engine = new DownloadEngine(transport);

        DownloadItem item = Item(Url, body, @"libraries\a\a\1.0\a-1.0.jar");
        item.FallbackUrls = new[] { MirrorUrl };

        DownloadOptions options = FastOptions();
        options.ProbeSourceSpeed = false;

        DownloadReport report = engine.EnsureAll(Plan(item), sandbox, options);

        Assert.IsTrue(report.IsComplete);
        Assert.AreEqual(0, transport.Requests.Count(r => string.Equals(r.Url, MirrorUrl, StringComparison.Ordinal)),
            "关掉探测就不该去碰备用源");
    }
    [TestMethod]
    public void EnsureAll_FailsOverToTheOfficialSourceWhenTheMirrorServesCorruptBytes()
    {
        // **这是"用镜像不降低完整性保证"那句话的直接验证。**
        // 镜像可以撒谎，但摘要来自官方元数据——撒谎只会让它的字节被丢弃并换源。
        // 反过来看：如果引擎信任了源声明的任何东西，这个用例就会拿到篡改内容。
        string sandbox = NewSandbox();
        byte[] good = Bytes("library-content");
        byte[] tampered = Bytes("TAMPERED-BY-A-BAD-MIRROR");

        FakeTransport transport = new FakeTransport();
        transport.Serve(Url, good);
        transport.Serve(MirrorUrl, tampered);

        // 让镜像显得更快，探测就会把它排到首位——于是第一次尝试必然落在坏源上
        transport.DelayMilliseconds = url => string.Equals(url, Url, StringComparison.Ordinal) ? 150 : 0;

        DownloadEngine engine = new DownloadEngine(transport);

        // 期望的摘要来自官方内容
        DownloadItem item = Item(Url, good, @"libraries\a\a\1.0\a-1.0.jar");
        item.FallbackUrls = new[] { MirrorUrl };

        DownloadOptions options = new DownloadOptions
        {
            MaxConcurrency = 1,
            MaxAttempts = 3,
            BaseRetryDelay = TimeSpan.Zero,
            MinimumFreeBytes = 0,
            ProbeSourceSpeed = true,
        };

        DownloadReport report = engine.EnsureAll(Plan(item), sandbox, options);

        Assert.IsTrue(report.IsComplete, "坏源必须被换掉，而不是让整次下载失败");

        string destination = Path.Combine(sandbox, @"libraries\a\a\1.0\a-1.0.jar");
        CollectionAssert.AreEqual(good, File.ReadAllBytes(destination), "落盘内容必须是官方内容，绝不是镜像给的坏字节");

        Assert.IsTrue(
            transport.Requests.Count(r => string.Equals(r.Url, MirrorUrl, StringComparison.Ordinal)) >= 2,
            "镜像应当被真正尝试过（探测一次 + 下载一次），否则这个用例什么都没证明");

        // 两次：第一次落在坏源上（摘要不符被丢弃），第二次换到官方源成功。
        // 这里刻意写死数字——它同时钉住"确实试过坏源"和"确实换源成功了"。
        Assert.AreEqual(2, report.Items[0].Attempts);
    }
}