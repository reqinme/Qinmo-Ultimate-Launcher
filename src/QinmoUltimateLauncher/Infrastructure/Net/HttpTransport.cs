using System;
using System.Collections.Generic;
using System.IO;
using System.Net;
using System.Text;
using System.Threading;
using Qul.Application.Ports;
using Qul.Domain.Diagnostics;

namespace Qul.Infrastructure.Net;

/// <summary>
/// 基于 <see cref="HttpWebRequest"/> 的传输实现。
///
/// **绝不安装证书校验回调。** 证书异常必须被分类上报（QUL-NET-0003），
/// 而不是被"忽略掉继续走"。这一点由单元测试断言
/// <see cref="ServicePointManager.ServerCertificateValidationCallback"/> 保持为 null 来守住。
/// </summary>
public sealed class HttpTransport : IHttpTransport
{
    private const int BufferHint = 81920;

    static HttpTransport()
    {
        // net48 在部分机器上默认不启用 TLS 1.2；显式补齐，避免整片 HTTPS 站点连不上。
        // 只做"补上"，不做任何降低校验强度的动作。
        ServicePointManager.SecurityProtocol |= SecurityProtocolType.Tls12;

        // **以下三项照搬 PCL2 的全局网络配置（Plain Craft Launcher 2, Application.xaml.vb:100-104）。**
        // 它们都不是"优化"，而是把 .NET Framework 的保守默认值调到与现代下载器相当。

        // 1) 每主机连接上限。**net48 的默认值是 2**（已在本机实测确认），
        //    意味着无论我们把并发开到多少，同一个主机上真正并行的连接最多只有 2 条。
        //    实测同一批真实资源对象：未设此项 232 KB/s，设为 10000 后 662–815 KB/s。
        ServicePointManager.DefaultConnectionLimit = 10000;

        // 2) 关掉 Expect: 100-continue。省掉每个请求一个往返；
        //    服务端几乎总会回 100，这个协商对小文件下载是纯开销。
        ServicePointManager.Expect100Continue = false;

        // 3) 关掉 Nagle 算法。下载是"发完请求就一直收"，不需要攒小包，
        //    开着它反而会给响应到达引入延迟。
        ServicePointManager.UseNagleAlgorithm = false;
    }

    public HttpFetchResponse Fetch(HttpFetchRequest request, CancellationToken cancellationToken)
    {
        if (request == null)
        {
            throw new ArgumentNullException(nameof(request));
        }

        if (string.IsNullOrWhiteSpace(request.Url))
        {
            throw new LauncherException(ErrorCode.NetUnreachable, "request url is empty");
        }

        HttpWebRequest webRequest = (HttpWebRequest)WebRequest.Create(request.Url);
        webRequest.Method = string.IsNullOrEmpty(request.Method) ? "GET" : request.Method.ToUpperInvariant();
        webRequest.AllowAutoRedirect = true;
        webRequest.Timeout = (int)Math.Min(int.MaxValue, request.Timeout.TotalMilliseconds);
        webRequest.ReadWriteTimeout = (int)Math.Min(int.MaxValue, request.Timeout.TotalMilliseconds);
        webRequest.UserAgent = "QinmoUltimateLauncher/0.1";
        webRequest.AutomaticDecompression = DecompressionMethods.None;

        ApplyProxy(webRequest, request.ProxyAddress);

        if (!string.IsNullOrEmpty(request.Accept))
        {
            webRequest.Accept = request.Accept;
        }

        foreach (KeyValuePair<string, string> header in request.Headers)
        {
            // 少数头（Host / Content-Type 等）不能走 Headers 集合，交给各自的属性。
            if (string.Equals(header.Key, "Content-Type", StringComparison.OrdinalIgnoreCase))
            {
                webRequest.ContentType = header.Value;
                continue;
            }

            webRequest.Headers[header.Key] = header.Value;
        }

        // RangeFrom 为 0 时，只有在同时给了终点的情况下才发 Range——
        // 否则会退回"从头取整个文件"，那正是既有续传路径要的行为。
        if (request.RangeFrom.HasValue
            && (request.RangeFrom.Value > 0 || request.RangeTo.HasValue))
        {
            long rangeFrom = request.RangeFrom.Value;

            if (request.RangeTo.HasValue && request.RangeTo.Value >= rangeFrom)
            {
                webRequest.AddRange(rangeFrom, request.RangeTo.Value);
            }
            else
            {
                webRequest.AddRange(rangeFrom);
            }
        }

        if (!string.IsNullOrEmpty(request.Body))
        {
            byte[] payload = Encoding.UTF8.GetBytes(request.Body!);
            webRequest.ContentType = request.ContentType ?? webRequest.ContentType ?? "application/json";
            webRequest.ContentLength = payload.Length;

            using (Stream body = webRequest.GetRequestStream())
            {
                body.Write(payload, 0, payload.Length);
            }
        }
        else if (!string.Equals(webRequest.Method, "GET", StringComparison.OrdinalIgnoreCase))
        {
            if (!string.IsNullOrEmpty(request.ContentType))
            {
                webRequest.ContentType = request.ContentType;
            }

            webRequest.ContentLength = 0;
        }

        cancellationToken.ThrowIfCancellationRequested();

        // **发请求之前就要挂上取消回调。**
        //
        // 先前只在拿到响应之后（IdleTimeoutStream 里）才注册，于是卡在
        // GetResponse() 里的请求取消不掉，只能等满 RequestTimeout。
        // 实测：点取消后界面停在「下载」整整 **61 秒**（RequestTimeout 是 60 秒），
        // 而下载其实在 3 秒内就停了——管线卡在等工作线程这件事上。
        CancellationTokenRegistration cancellation = cancellationToken.CanBeCanceled
            ? cancellationToken.Register(AbortRequest, webRequest)
            : default(CancellationTokenRegistration);

        try
        {
            HttpWebResponse response = (HttpWebResponse)webRequest.GetResponse();

            // 响应流的读取由 IdleTimeoutStream 自己挂着取消回调，这里可以放手。
            cancellation.Dispose();

            return BuildResponse(response, request, webRequest, cancellationToken);
        }
        catch (WebException ex)
        {
            cancellation.Dispose();

            HttpWebResponse? errorResponse = ex.Response as HttpWebResponse;
            if (errorResponse != null)
            {
                try
                {
                    return BuildResponse(errorResponse, request, webRequest, cancellationToken);
                }
                catch (Exception)
                {
                    errorResponse.Dispose();
                }
            }

            throw Classify(ex);
        }
    }

    /// <summary>
    /// 取消时掐断请求。**必须能在拿到响应之前也生效**——
    /// 否则取消的实际延迟就等于请求超时（默认 60 秒）。
    /// </summary>
    private static void AbortRequest(object? state)
    {
        try
        {
            ((HttpWebRequest)state!).Abort();
        }
        catch (ObjectDisposedException)
        {
            // 请求已经被正常释放，说明这次取消来晚了。
        }
    }

    private static void ApplyProxy(HttpWebRequest request, string? proxyAddress)
    {
        if (proxyAddress == null)
        {
            // 跟随系统代理：用默认设置，不动它。
            request.Proxy = WebRequest.DefaultWebProxy;
            return;
        }

        if (proxyAddress.Length == 0)
        {
            // 显式直连。必须与 null 区分，否则用户"关掉代理"的意图表达不出来。
            request.Proxy = null;
            return;
        }

        try
        {
            request.Proxy = new WebProxy(proxyAddress.Trim());
        }
        catch (UriFormatException ex)
        {
            throw new LauncherException(ErrorCode.NetProxyInvalid, "malformed proxy address", ex);
        }
    }

    private static HttpFetchResponse BuildResponse(HttpWebResponse response, HttpFetchRequest request, HttpWebRequest webRequest, CancellationToken cancellationToken)
    {
        HttpStatusCode statusCode = response.StatusCode;
        long? contentLength = response.ContentLength >= 0 ? response.ContentLength : (long?)null;
        long? rangeStart = null;
        long? totalLength = contentLength;

        if (statusCode == HttpStatusCode.PartialContent)
        {
            // Content-Range: bytes 100-999/1000
            string? contentRange = response.Headers["Content-Range"];
            if (!string.IsNullOrEmpty(contentRange))
            {
                ParseContentRange(contentRange!, out rangeStart, out totalLength);
            }
        }

        HttpFetchStatus status;
        if (statusCode == HttpStatusCode.OK)
        {
            status = HttpFetchStatus.Success;
            totalLength = contentLength;
        }
        else if (statusCode == HttpStatusCode.PartialContent)
        {
            status = HttpFetchStatus.PartialContent;
        }
        else if (statusCode == HttpStatusCode.RequestedRangeNotSatisfiable)
        {
            status = HttpFetchStatus.RangeNotSatisfiable;
        }
        else if (statusCode == HttpStatusCode.NotFound)
        {
            status = HttpFetchStatus.NotFound;
        }
        else if ((int)statusCode >= 500)
        {
            status = HttpFetchStatus.ServerError;
        }
        else
        {
            status = HttpFetchStatus.Other;
        }

        // 服务端返回 200 却带着 Range 请求，说明它忽略了 Range，必须从头写而不是追加。
        if (request.RangeFrom.HasValue && request.RangeFrom.Value > 0 && status == HttpFetchStatus.Success)
        {
            rangeStart = 0;
        }

        return new HttpFetchResponse
        {
            Status = status,
            StatusCode = (int)statusCode,
            ContentLength = contentLength,
            TotalLength = totalLength,
            RangeStart = rangeStart,
            // **读取必须带空闲看门狗。**
            // 实测踩到过：一个 0 字节的 .part 停在那里 20 分钟，
            // 其余 worker 早已退出，只剩一个线程阻塞在读取上，
            // 而 RequestTimeout 设的 60 秒并没有让 ReadWriteTimeout 触发。
            // 那才是先前"完整安装跑不完"的真正原因——不是网络慢，是卡住了。
            Content = new IdleTimeoutStream(response.GetResponseStream(), webRequest, IdleTimeout(request), cancellationToken),
        };
    }

    /// <summary>
    /// 空闲上限：多久没有新数据就认为这条连接已经死了。
    ///
    /// 取请求超时本身，但**最多 60 秒**——挂死 60 秒与挂死 20 分钟是决定性的差别。
    /// 不低于 5 秒，免得把正常的慢响应误判成死连接。
    /// </summary>
    private static TimeSpan IdleTimeout(HttpFetchRequest request)
    {
        double seconds = Math.Min(request.Timeout.TotalSeconds, 60);
        return TimeSpan.FromSeconds(Math.Max(5, seconds));
    }

    private static void ParseContentRange(string header, out long? rangeStart, out long? totalLength)
    {
        rangeStart = null;
        totalLength = null;

        // bytes <start>-<end>/<total>，total 可能是 '*'
        int space = header.IndexOf(' ');
        if (space < 0)
        {
            return;
        }

        string rest = header.Substring(space + 1);
        int dash = rest.IndexOf('-');
        int slash = rest.IndexOf('/');
        if (dash <= 0 || slash < 0)
        {
            return;
        }

        if (long.TryParse(rest.Substring(0, dash), out long start))
        {
            rangeStart = start;
        }

        string totalText = rest.Substring(slash + 1);
        if (long.TryParse(totalText, out long total))
        {
            totalLength = total;
        }
    }

    /// <summary>
    /// 把底层异常翻译成错误码。重试决策只依赖错误码，不依赖异常细节。
    /// </summary>
    private static LauncherException Classify(WebException ex)
    {
        // 证书类失败必须先判：它常被包在 ProtocolError 里，若先按状态码归类就会被误判成"服务器错误"而重试。
        if (IsCertificateFailure(ex))
        {
            return new LauncherException(ErrorCode.NetCertificateInvalid, "TLS certificate validation failed", ex);
        }

        switch (ex.Status)
        {
            case WebExceptionStatus.Timeout:
                return new LauncherException(ErrorCode.NetTimeout, "request timed out", ex);

            case WebExceptionStatus.NameResolutionFailure:
            case WebExceptionStatus.ConnectFailure:
            case WebExceptionStatus.ReceiveFailure:
            case WebExceptionStatus.SendFailure:
            case WebExceptionStatus.PipelineFailure:
                return new LauncherException(ErrorCode.NetUnreachable, "network unreachable", ex);

            case WebExceptionStatus.ProxyNameResolutionFailure:
                return new LauncherException(ErrorCode.NetProxyInvalid, "proxy name resolution failed", ex);

            default:
                if (ex.Status == WebExceptionStatus.ProtocolError && ex.Response is HttpWebResponse)
                {
                    // 响应已在上层处理；走到这里说明构建响应本身失败了。
                    return new LauncherException(ErrorCode.NetHttpStatus, "protocol error", ex);
                }

                return new LauncherException(ErrorCode.NetUnreachable, "web request failed: " + ex.Status, ex);
        }
    }

    private static bool IsCertificateFailure(WebException ex)
    {
        if (ex.Status == WebExceptionStatus.TrustFailure ||
            ex.Status == WebExceptionStatus.SecureChannelFailure)
        {
            return true;
        }

        for (Exception? inner = ex.InnerException; inner != null; inner = inner.InnerException)
        {
            if (inner is System.Security.Authentication.AuthenticationException)
            {
                return true;
            }
        }

        return false;
    }
}
