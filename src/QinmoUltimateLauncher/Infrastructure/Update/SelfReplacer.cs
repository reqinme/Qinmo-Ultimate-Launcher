using System;
using System.IO;
using Qul.Application.Update;

namespace Qul.Infrastructure.Update;

/// <summary>
/// 自替换：把正在运行的可执行文件换成新版本。
///
/// **刻意做成纯文件路径操作，不读"正在运行的自己"。** 这样它可以被完整地测试——
/// 传三个临时路径进来就能验证全部分支，唯一的不可测部分被压缩到两个 <see cref="File.Move(string,string)"/>。
///
/// Windows 允许对正在运行的 exe **改名**（只是不允许覆盖或删除），所以整个替换不需要第二个进程。
/// 先把自己改名成备份，再把新文件移到原路径。
///
/// 第二步失败时进程仍然活着，**当场把备份改回来**——用户拿到的是完好无损的旧版本，
/// 而不是一个打不开的快捷方式。
/// </summary>
public static class SelfReplacer
{
    /// <summary>把一个文件换到另一个位置。</summary>
    public static SelfReplaceOutcome Apply(string selfPath, string newPath, string backupPath)
    {
        if (string.IsNullOrWhiteSpace(selfPath))
        {
            throw new ArgumentException("self path is required", nameof(selfPath));
        }

        if (string.IsNullOrWhiteSpace(newPath))
        {
            throw new ArgumentException("new path is required", nameof(newPath));
        }

        if (string.IsNullOrWhiteSpace(backupPath))
        {
            throw new ArgumentException("backup path is required", nameof(backupPath));
        }

        if (!File.Exists(selfPath))
        {
            return SelfReplaceOutcome.Failed("主程序文件不存在：" + selfPath);
        }

        if (!File.Exists(newPath))
        {
            return SelfReplaceOutcome.Failed("待应用的新版本不存在：" + newPath);
        }

        // 第一步：自身改名。运行中的 exe 允许改名，这是整个方案成立的前提。
        try
        {
            if (File.Exists(backupPath))
            {
                File.Delete(backupPath);
            }

            File.Move(selfPath, backupPath);
        }
        catch (Exception ex) when (ex is IOException || ex is UnauthorizedAccessException)
        {
            return SelfReplaceOutcome.Failed("备份原文件失败：" + ex.Message);
        }

        // 第二步：新文件就位。
        try
        {
            File.Move(newPath, selfPath);
            return SelfReplaceOutcome.Ok();
        }
        catch (Exception ex) when (ex is IOException || ex is UnauthorizedAccessException)
        {
            // 关键：进程还活着，能自己收拾。绝不留一个打不开的快捷方式。
            try
            {
                File.Move(backupPath, selfPath);
                return SelfReplaceOutcome.FailedAndRestored("应用新版本失败，已恢复原版本：" + ex.Message);
            }
            catch (Exception inner) when (inner is IOException || inner is UnauthorizedAccessException)
            {
                return SelfReplaceOutcome.Failed(
                    "应用新版本失败，且恢复也失败：" + ex.Message + " / " + inner.Message);
            }
        }
    }

    /// <summary>
    /// 回退：把备份换回原路径，把失败的新版本挪到一边。
    ///
    /// 由**新版本自己**在启动时调用——它虽然坏了，但改名自己仍然是允许的。
    /// </summary>
    public static SelfReplaceOutcome Rollback(string selfPath, string backupPath, string discardPath)
    {
        if (!File.Exists(backupPath))
        {
            return SelfReplaceOutcome.Failed("没有可回退的备份：" + backupPath);
        }

        try
        {
            if (File.Exists(selfPath))
            {
                if (File.Exists(discardPath))
                {
                    File.Delete(discardPath);
                }

                File.Move(selfPath, discardPath);
            }

            File.Move(backupPath, selfPath);
            return SelfReplaceOutcome.Ok();
        }
        catch (Exception ex) when (ex is IOException || ex is UnauthorizedAccessException)
        {
            return SelfReplaceOutcome.Failed("回退失败：" + ex.Message);
        }
    }
}
