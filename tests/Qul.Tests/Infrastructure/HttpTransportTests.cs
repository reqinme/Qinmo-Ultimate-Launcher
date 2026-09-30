using System.Net;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Infrastructure.Net;

namespace Qul.Tests.Infrastructure;

/// <summary>
/// 传输层的全局网络配置。
///
/// 这三项都不是"优化"，而是把 .NET Framework 的保守默认值调到与现代下载器相当。
/// **它们是实测出来的**：同一批真实资源对象（1.7.10 的资源索引抽样），
/// 未设置时为 232 KB/s，设置后为 662–815 KB/s，接近 3 倍。
/// </summary>
[TestClass]
public sealed class HttpTransportTests
{
    [TestMethod]
    public void ConnectionLimit_IsRaisedFarAboveTheFrameworkDefaultOfTwo()
    {
        // .NET Framework 每主机连接上限的默认值是 2（本机实测确认）。
        // **不改它，无论把并发开到多少，同一个主机上真正并行的连接最多只有 2 条。**
        // PCL2 把它设为 10000（Application.xaml.vb:101），我们照搬。
        //
        // **必须实例化才会触发静态构造**：typeof() 是编译期的，不会运行静态构造函数。
        HttpTransport transport = new HttpTransport();
        Assert.IsNotNull(transport);

        Assert.IsTrue(
            ServicePointManager.DefaultConnectionLimit >= 1000,
            "每主机连接上限必须远高于默认值 2，否则并发下载名不副实");
    }

    [TestMethod]
    public void Expect100Continue_IsDisabledToSaveARoundTripPerRequest()
    {
        // 服务端几乎总会回 100 Continue，这个协商对小文件下载是纯开销。
        // **必须实例化才会触发静态构造**：typeof() 不会运行静态构造函数，
        // 用它触发的话断言到的是框架默认值，测试会假绿。
        HttpTransport transport = new HttpTransport();
        Assert.IsNotNull(transport);

        Assert.IsFalse(ServicePointManager.Expect100Continue);
    }

    [TestMethod]
    public void Nagle_IsDisabledBecauseDownloadsDoNotNeedPacketCoalescing()
    {
        // 下载是"发完请求就一直收"，不需要攒小包；开着它反而给响应到达引入延迟。
        HttpTransport transport = new HttpTransport();
        Assert.IsNotNull(transport);

        Assert.IsFalse(ServicePointManager.UseNagleAlgorithm);
    }

    [TestMethod]
    public void CertificateValidationCallback_StaysNull()
    {
        // 安全基线：证书异常必须被分类上报，绝不静默绕过。
        // 网络配置怎么调都不该动到这一项。
        Assert.IsNull(ServicePointManager.ServerCertificateValidationCallback);
    }
}
