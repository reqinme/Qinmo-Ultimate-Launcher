using System;
using System.Runtime.InteropServices;

namespace Qul.Infrastructure.Platform;

/// <summary>
/// 把启动后不再使用的页交还给操作系统。
///
/// **为什么需要它（实测依据）：**
/// 启动时会临时分配大量对象（版本清单与资源索引的 JSON DOM），
/// 而 .NET 的 GC 在没有内存压力时并不把内存还给系统。
///
/// 实测（GUI 启动后空闲 20 秒，再用 <c>EmptyWorkingSet</c> 观察）：
///   清空前工作集  226.9 MB
///   清空后立即     11.9 MB
///   空闲 20 秒后   23.7 MB   ← 只长回这么多
///
/// 也就是说 **227 MB 里只有约 24 MB 是启动后真正还会被触碰的页**，
/// 其余约 200 MB 一直被占着，而它并不会再被用到。
///
/// **这不是把数字做小的花招：** 页会被移到系统的备用链表，
/// 应用再次需要时由系统重新调入（代价是一次缺页）。真正省下的是
/// "启动器开着不动时对其他程序的内存压力"——而启动器恰恰常年就是开着不动。
///
/// 因此只在**启动完成且界面进入空闲后**做一次，绝不放在热路径上，
/// 也绝不在游戏运行期间反复做。
/// </summary>
public static class WorkingSetTrimmer
{
    private static bool _alreadyFailed;

    /// <summary>
    /// 先真正回收一次垃圾，再把空闲页交还系统。
    /// 任何一步失败都静默跳过——这只是省内存，不该影响任何功能。
    /// </summary>
    public static void Trim()
    {
        if (_alreadyFailed)
        {
            return;
        }

        try
        {
            // 启动期的 DOM 是垃圾，但 GC 未必已经收过它们。
            // 先收干净，再交还——否则交还的只是"暂时没被碰"的活对象页，
            // 下一次 GC 前后又会长回来。
            GC.Collect(2, GCCollectionMode.Forced, blocking: true);
            GC.WaitForPendingFinalizers();
            GC.Collect(2, GCCollectionMode.Forced, blocking: true);

            // (IntPtr)(-1) 表示"由系统决定合适的工作集大小"，
            // 即把空闲页移出工作集——不是把页丢掉。
            SetProcessWorkingSetSize(GetCurrentProcess(), new IntPtr(-1), new IntPtr(-1));
        }
        catch (Exception ex) when (
            ex is DllNotFoundException
            || ex is EntryPointNotFoundException
            || ex is InvalidOperationException)
        {
            // 某些受限环境下拿不到这些入口点。省内存失败不影响功能，也不再重试。
            _alreadyFailed = true;
        }
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr GetCurrentProcess();

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetProcessWorkingSetSize(IntPtr process, IntPtr minimum, IntPtr maximum);
}
