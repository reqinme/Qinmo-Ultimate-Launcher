using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;

namespace Qul.Application.Ports;

public enum HttpFetchStatus
{
    /// <summary>200，拿到了完整内容。</summary>
    Success,

    /// <summary>206，续传成功，响应体是请求区间。</summary>
    PartialContent,

    /// <summary>416，本地残留比远端还长，必须丢弃残留重头来。</summary>
    RangeNotSatisfiable,

    NotFound,

    ServerError,

    /// <summary>其他非成功状态。</summary>
    Other,
}

public sealed class HttpFetchRequest
{
    public string Url { get; set; } = string.Empty;

    /// <summary>HTTP 方法。认证流程需要 POST。</summary>
    public string Method { get; set; } = "GET";

    /// <summary>请求体。为 null 表示无包体。</summary>
    public string? Body { get; set; }

    public string? ContentType { get; set; }

    public string? Accept { get; set; }

    /// <summary>自定义请求头，例如 Authorization 与 x-xbl-contract-version。</summary>
    public IReadOnlyDictionary<string, string> Headers { get; set; } =
        new Dictionary<string, string>(0, StringComparer.Ordinal);

    /// <summary>续传起点。null 表示从头取。</summary>
    public long? RangeFrom { get; set; }

    /// <summary>
    /// null = 跟随系统代理；空串 = 直连；其他 = 显式代理地址。
    /// 三者必须有区别，否则"关闭代理"这个用户意图无法表达。
    /// </summary>
    public string? ProxyAddress { get; set; }

    public TimeSpan Timeout { get; set; } = TimeSpan.FromSeconds(30);
}

public sealed class HttpFetchResponse : IDisposable
{
    public HttpFetchStatus Status { get; set; }

    public int StatusCode { get; set; }

    /// <summary>本次响应体的长度。未知时为 null。</summary>
    public long? ContentLength { get; set; }

    /// <summary>资源总长度（来自 Content-Range 或 Content-Length）。未知时为 null。</summary>
    public long? TotalLength { get; set; }

    /// <summary>响应体实际起始偏移。用于确认服务端真的满足了 Range 请求。</summary>
    public long? RangeStart { get; set; }

    public Stream Content { get; set; } = Stream.Null;

    public void Dispose()
    {
        Content?.Dispose();
    }
}

/// <summary>
/// HTTP 传输端口。
///
/// 失败一律抛 <c>LauncherException</c> 且错误码已分类（QUL-NET-0001..0005）——
/// 分类发生在能看到底层异常细节的基础设施里，编排侧只按错误码决策是否重试。
///
/// 之所以把它做成端口：续传、重试、校验这些行为必须能被伪造传输完整单测。
/// 真实网络测不出"拔网线"。
/// </summary>
public interface IHttpTransport
{
    HttpFetchResponse Fetch(HttpFetchRequest request, CancellationToken cancellationToken);
}
