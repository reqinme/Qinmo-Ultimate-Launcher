using System.Net;
using System;
using System.Diagnostics;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using System.IO;
using Qul.Application.Ports;
using Qul.Domain.Diagnostics;
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
    [TestMethod]
    public void Fetch_DoesNotHangForeverWhenTheServerSendsHeadersButNoBody()
    {
        // **这条用例守的是一个真实的挂死，不是假想。**
        //
        // 实测：缓存里出现一个 0 字节的 .part，停在那里 20 分钟，
        // 其余 worker 早已退出，只剩一个线程阻塞在读取上——
        // 整次安装永久挂死，而当时 RequestTimeout 设的是 60 秒。
        // **那也是先前几次"完整安装跑不完"的真正原因：不是网络慢，是卡住了。**
        //
        // 复现方式：本地服务器**发完响应头就沉默**。
        // 这样 GetResponse() 会立刻返回（走不到 HttpWebRequest.Timeout），
        // 阻塞只可能发生在读正文上——也就是空闲看门狗唯一能救的场景。
        TcpListener listener = new TcpListener(IPAddress.Loopback, 0);
        listener.Start();
        int port = ((IPEndPoint)listener.LocalEndpoint).Port;

        Task silence = Task.Run(
            () =>
            {
                try
                {
                    using (TcpClient client = listener.AcceptTcpClient())
                    using (NetworkStream stream = client.GetStream())
                    {
                        byte[] headers = Encoding.ASCII.GetBytes(
                            "HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n");

                        stream.Write(headers, 0, headers.Length);
                        stream.Flush();

                        // 头发了，正文一个字节都不发。
                        Thread.Sleep(TimeSpan.FromSeconds(30));
                    }
                }
                catch (SocketException)
                {
                }
                catch (ObjectDisposedException)
                {
                }
            });

        try
        {
            HttpTransport transport = new HttpTransport();
            Stopwatch watch = Stopwatch.StartNew();

            using (HttpFetchResponse response = transport.Fetch(
                new HttpFetchRequest
                {
                    Url = "http://127.0.0.1:" + port + "/never",
                    Timeout = TimeSpan.FromSeconds(3),
                },
                CancellationToken.None))
            {
                byte[] buffer = new byte[1024];
                Exception? caught = null;

                try
                {
                    response.Content.Read(buffer, 0, buffer.Length);
                }
                catch (Exception ex)
                {
                    caught = ex;
                }

                watch.Stop();

                Assert.IsNotNull(caught, "对端不再发数据时必须抛出来，而不是永久阻塞");
                Assert.IsTrue(
                    watch.Elapsed < TimeSpan.FromSeconds(20),
                    "必须在空闲上限内放弃；实际阻塞了 " + watch.Elapsed.TotalSeconds.ToString("0.0") + " 秒");
            }
        }
        finally
        {
            listener.Stop();
        }
    }
}