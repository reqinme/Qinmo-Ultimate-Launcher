using System;
using System.Collections.Generic;
using System.IO;
using System.IO.Compression;
using System.Text;
using System.Threading;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Application.Ports;
using Qul.Domain.Diagnostics;

namespace Qul.Tests.Downloads;

/// <summary>
/// 伪造传输。真实网络测不出"拔网线"，所以续传、重试、校验这些行为必须靠它来验。
/// 默认按标准 Range 语义服务内容（206 + Content-Range），可以像真服务器一样被续传。
/// </summary>
internal sealed class FakeTransport : IHttpTransport
{
    private readonly Dictionary<string, byte[]> _content = new Dictionary<string, byte[]>(StringComparer.Ordinal);

    /// <summary>
    /// 收到过的请求。
    ///
    /// **写入必须加锁。** 分段下载会从多个任务并发调用 <see cref="Fetch"/>，
    /// 而 <see cref="List{T}"/> 的 Add 不是线程安全的：并发写会**丢条目**
    /// （测试数出来的请求数偏少），偶尔还会让某个槽位仍是 null
    /// （读取方的 lambda 直接 NullReferenceException）。
    /// 这两种表现都是"间歇失败"，而间歇失败的测试很快就会被所有人忽略。
    /// </summary>
    public List<HttpFetchRequest> Requests { get; } = new List<HttpFetchRequest>();

    private readonly object _requestsGate = new object();

    /// <summary>按调用序号注入故障。返回 null 表示交给默认逻辑。</summary>
    public Func<HttpFetchRequest, int, HttpFetchResponse?>? Interceptor { get; set; }

    /// <summary>
    /// 按 URL 注入延迟（毫秒），用来模拟"慢但能通"的源。
    /// 只按失败换源是测不出慢源的——它永远不会失败，只会一直磨。
    /// </summary>
    public Func<string, int>? DelayMilliseconds { get; set; }

    /// <summary>
    /// 按**整个请求**决定延迟，而不是只看 URL。
    ///
    /// 需要它是为了能只拖慢**分段请求**（<c>RangeFrom &gt; 0</c>）——
    /// 这样取消才会落在某个分段任务**内部**，而不是在分段开始之前就被发现。
    /// 两者走的是完全不同的代码路径，只有前者能判别"分段任务里的取消"。
    /// </summary>
    public Func<HttpFetchRequest, int>? DelayForRequest { get; set; }

    /// <summary>
    /// 设为 true 时忽略 Range，一律回 200 + 整个文件——
    /// 用来验证分段下载能识别"服务端不支持 Range"并回退单连接。
    /// </summary>
    public bool IgnoreRangeRequests { get; set; }

    public void Serve(string url, byte[] content)
    {
        _content[url] = content;
    }

    public HttpFetchResponse Fetch(HttpFetchRequest request, CancellationToken cancellationToken)
    {
        int call;

        lock (_requestsGate)
        {
            Requests.Add(request);
            call = Requests.Count;
        }

        int delay = DelayForRequest?.Invoke(request) ?? DelayMilliseconds?.Invoke(request.Url) ?? 0;
        if (delay > 0)
        {
            Thread.Sleep(delay);
        }

        if (Interceptor != null)
        {
            HttpFetchResponse? injected = Interceptor(request, call);
            if (injected != null)
            {
                return injected;
            }
        }

        if (!_content.TryGetValue(request.Url, out byte[]? body))
        {
            return new HttpFetchResponse { Status = HttpFetchStatus.NotFound, StatusCode = 404, Content = Stream.Null };
        }

        long from = request.RangeFrom ?? 0;
        bool wantsRange = request.RangeFrom.HasValue || request.RangeTo.HasValue;

        if (IgnoreRangeRequests && wantsRange)
        {
            // 200 + 整个文件：Range 被无视了。
            return new HttpFetchResponse
            {
                Status = HttpFetchStatus.Success,
                StatusCode = 200,
                ContentLength = body.Length,
                TotalLength = body.Length,
                Content = new MemoryStream(body),
            };
        }

        // **等于也算**：区间起点恰好落在文件末尾时，真实服务器回 416
        // （"你要的那一段不存在"），而不是回 200。假传输要照实模拟，
        // 否则"本地已有完整文件"这条路径在测试里根本走不到。
        if (from >= body.Length)
        {
            return new HttpFetchResponse { Status = HttpFetchStatus.RangeNotSatisfiable, StatusCode = 416, Content = Stream.Null };
        }

        long to = request.RangeTo.HasValue
            ? Math.Min(request.RangeTo.Value, body.Length - 1)
            : body.Length - 1;

        if (wantsRange && to >= from)
        {
            int length = (int)(to - from + 1);
            byte[] slice = new byte[length];
            Array.Copy(body, from, slice, 0, length);

            return new HttpFetchResponse
            {
                Status = HttpFetchStatus.PartialContent,
                StatusCode = 206,
                ContentLength = slice.Length,
                TotalLength = body.Length,
                RangeStart = from,
                Content = new MemoryStream(slice),
            };
        }

        return new HttpFetchResponse
        {
            Status = HttpFetchStatus.Success,
            StatusCode = 200,
            ContentLength = body.Length,
            TotalLength = body.Length,
            RangeStart = 0,
            Content = new MemoryStream(body),
        };
    }

    /// <summary>只吐出前 N 个字节然后抛 IOException，模拟传输中途被掐断。</summary>
    public static HttpFetchResponse Truncated(byte[] body, int failAfter, long rangeFrom = 0)
    {
        return new HttpFetchResponse
        {
            Status = rangeFrom > 0 ? HttpFetchStatus.PartialContent : HttpFetchStatus.Success,
            StatusCode = rangeFrom > 0 ? 206 : 200,
            ContentLength = body.Length - rangeFrom,
            TotalLength = body.Length,
            RangeStart = rangeFrom,
            Content = new FaultingStream(body, (int)rangeFrom, failAfter),
        };
    }

    private sealed class FaultingStream : Stream
    {
        private readonly byte[] _data;
        private readonly int _failAt;
        private int _position;

        public FaultingStream(byte[] data, int start, int failAt)
        {
            _data = data;
            _position = start;
            _failAt = failAt;
        }

        public override bool CanRead => true;

        public override bool CanSeek => false;

        public override bool CanWrite => false;

        public override long Length => throw new NotSupportedException();

        public override long Position
        {
            get => _position;
            set => throw new NotSupportedException();
        }

        public override int Read(byte[] buffer, int offset, int count)
        {
            if (_position >= _failAt)
            {
                throw new IOException("simulated connection reset");
            }

            int allowed = Math.Min(count, _failAt - _position);
            allowed = Math.Min(allowed, _data.Length - _position);

            if (allowed <= 0)
            {
                return 0;
            }

            Array.Copy(_data, _position, buffer, offset, allowed);
            _position += allowed;
            return allowed;
        }

        public override void Flush()
        {
        }

        public override long Seek(long offset, SeekOrigin origin)
        {
            throw new NotSupportedException();
        }

        public override void SetLength(long value)
        {
            throw new NotSupportedException();
        }

        public override void Write(byte[] buffer, int offset, int count)
        {
            throw new NotSupportedException();
        }
    }
}

/// <summary>
/// 同步记录进度。
/// 刻意不用 <c>Progress&lt;T&gt;</c>：它依赖同步上下文投递，在无上下文的测试里
/// 断言执行时回调可能还没跑，会得到"看起来通过"的假绿。
/// </summary>
internal sealed class RecordingProgress : System.IProgress<Qul.Domain.Downloads.DownloadProgress>
{
    public List<Qul.Domain.Downloads.DownloadProgress> Updates { get; } =
        new List<Qul.Domain.Downloads.DownloadProgress>();

    public void Report(Qul.Domain.Downloads.DownloadProgress value)
    {
        lock (Updates)
        {
            Updates.Add(value);
        }
    }
}
