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

    public List<HttpFetchRequest> Requests { get; } = new List<HttpFetchRequest>();

    /// <summary>按调用序号注入故障。返回 null 表示交给默认逻辑。</summary>
    public Func<HttpFetchRequest, int, HttpFetchResponse?>? Interceptor { get; set; }

    /// <summary>
    /// 按 URL 注入延迟（毫秒），用来模拟"慢但能通"的源。
    /// 只按失败换源是测不出慢源的——它永远不会失败，只会一直磨。
    /// </summary>
    public Func<string, int>? DelayMilliseconds { get; set; }

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
        Requests.Add(request);
        int call = Requests.Count;

        int delay = DelayMilliseconds?.Invoke(request.Url) ?? 0;
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

        if (from > body.Length)
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
