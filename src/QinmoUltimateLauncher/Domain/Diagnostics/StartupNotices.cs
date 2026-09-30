using System;
using System.Collections.Generic;

namespace Qul.Domain.Diagnostics;

/// <summary>
/// 启动期间就发生、且**必须让用户知道**的事，拼成一段人话。
///
/// 目前两类：
/// <list type="bullet">
/// <item>数据根建不出来 —— 先前只写进日志，而**数据根建不出来时日志本身也写不出来**，
/// 于是用户完全不知道数据去了哪里。</item>
/// <item>更新已回退 —— 用户眼前跑的是刚被判定不健康的那一版，而磁盘上已经换回去了；
/// 不说的话，下次启动"版本倒退"会毫无解释。</item>
/// </list>
///
/// 做成纯函数是为了能直接测：界面与命令行**共用同一段拼装**，
/// 否则迟早出现"命令行说回退了、界面没说"这种最难查的分歧。
/// </summary>
public static class StartupNotices
{
    /// <summary>没有要说的就返回空串。</summary>
    public static string Build(ErrorCode? dataRootWarning, string? updateNotice)
    {
        List<string> parts = new List<string>();

        if (dataRootWarning.HasValue)
        {
            parts.Add(
                ErrorCodes.Id(dataRootWarning.Value) + " " + ErrorCodes.Hint(dataRootWarning.Value)
                + " 数据可能无法保存，请检查程序目录是否可写。");
        }

        if (!string.IsNullOrWhiteSpace(updateNotice))
        {
            parts.Add(updateNotice!.Trim());
        }

        return string.Join(Environment.NewLine, parts);
    }
}
