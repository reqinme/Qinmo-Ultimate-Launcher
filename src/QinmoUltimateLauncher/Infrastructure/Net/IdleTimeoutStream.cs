using System;
using System.IO;
using System.Net;
using System.Threading;

namespace Qul.Infrastructure.Net;

/// <summary>
/// 盯着"多久没有新数据"的读取包装。
///
/// **为什么需要它：** .NET Framework 的 <c>HttpWebRequest.ReadWriteTimeout</c>
/// 在"连接还在、对端却不再发数据"时并不总会触发。
///
/// 实测踩到过：一个 0 字节的 <c>.part</c> 停在那里 **20 分钟**，
/// 把整次安装永久挂死——其余 worker 早已退出，只剩一个线程阻塞在读取上，
/// 而当时 <c>RequestTimeout</c> 设的是 60 秒。这也是先前几次"完整安装跑不完"
/// 的真正原因：**不是网络慢，是卡住了。**
///
/// 做法：每次读取前武装一个计时器，读到数据就解除；超时则 <c>Abort</c> 请求，
/// 让正在阻塞的读取抛出来，从而落进引擎既有的重试与换源路径。
/// **"挂死"与"失败后重试"的分界就在这一点上。**
/// </summary>
internal sealed class IdleTimeoutStream : Stream
{
    private readonly Stream _inner;
    private readonly HttpWebRequest _request;
    private readonly Timer _timer;
    private readonly TimeSpan _idleTimeout;
    private bool _disposed;

    public IdleTimeoutStream(Stream inner, HttpWebRequest request, TimeSpan idleTimeout)
    {
        _inner = inner ?? throw new ArgumentNullException(nameof(inner));
        _request = request ?? throw new ArgumentNullException(nameof(request));

        if (idleTimeout <= TimeSpan.Zero)
        {
            throw new ArgumentOutOfRangeException(nameof(idleTimeout));
        }

        _idleTimeout = idleTimeout;
        _timer = new Timer(OnIdle, null, Timeout.Infinite, Timeout.Infinite);
    }

    public override bool CanRead => _inner.CanRead;

    public override bool CanSeek => false;

    public override bool CanWrite => false;

    public override long Length => _inner.Length;

    public override long Position
    {
        get => throw new NotSupportedException();
        set => throw new NotSupportedException();
    }

    public override int Read(byte[] buffer, int offset, int count)
    {
        _timer.Change(_idleTimeout, Timeout.InfiniteTimeSpan);

        try
        {
            return _inner.Read(buffer, offset, count);
        }
        finally
        {
            // 无论读到数据还是抛异常，都要解除——
            // 否则下一次读取还没开始就被上一次的计时器掐掉。
            _timer.Change(Timeout.Infinite, Timeout.Infinite);
        }
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

    protected override void Dispose(bool disposing)
    {
        if (disposing && !_disposed)
        {
            _disposed = true;
            _timer.Dispose();
            _inner.Dispose();
        }

        base.Dispose(disposing);
    }

    private void OnIdle(object? state)
    {
        // 主动掐断。正在阻塞的读取会因此抛出，转而走重试与换源。
        try
        {
            _request.Abort();
        }
        catch (ObjectDisposedException)
        {
            // 请求已经被正常释放，说明这次只是计时器来晚了。
        }
    }
}
