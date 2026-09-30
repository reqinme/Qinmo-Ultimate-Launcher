using System;
using System.Collections.Generic;
using System.Text;

namespace Qul.Domain.Launch;

/// <summary>
/// 按 Windows 的规则把参数列表拼成命令行。
///
/// 这不是"有空格就加引号"那么简单。类路径里会出现形如
/// <c>C:\Users\some user\Documents\DeepSeek desktop\...</c> 的路径——含空格，
/// 也可能以反斜杠结尾（Java 的 <c>-cp</c> 就常以分隔符结尾）。
/// 引号规则搞错，结果就是"启动失败且看不出原因"：JVM 会把路径截断成两半。
///
/// 规则（与 .NET 自身的 argv 解析、以及 C 运行时一致）：
///   1) 不含空格、制表符、双引号 → 原样输出；
///   2) 否则整体加引号，其中：
///      - 反斜杠只在"后面紧跟双引号"或"位于参数末尾"时才需要翻倍；
///      - 参数内部的双引号写成 \"，且它前面的反斜杠要翻倍。
/// </summary>
public static class CommandLineBuilder
{
    public static string Build(IReadOnlyList<string> arguments)
    {
        if (arguments == null || arguments.Count == 0)
        {
            return string.Empty;
        }

        StringBuilder builder = new StringBuilder();

        for (int i = 0; i < arguments.Count; i++)
        {
            if (i > 0)
            {
                builder.Append(' ');
            }

            builder.Append(Quote(arguments[i] ?? string.Empty));
        }

        return builder.ToString();
    }

    public static string Quote(string argument)
    {
        if (argument == null)
        {
            return "\"\"";
        }

        if (argument.Length > 0 && argument.IndexOfAny(NeedsQuoting) < 0)
        {
            return argument;
        }

        StringBuilder builder = new StringBuilder(argument.Length + 8);
        builder.Append('"');

        for (int i = 0; i < argument.Length; i++)
        {
            int backslashes = 0;
            while (i < argument.Length && argument[i] == '\\')
            {
                backslashes++;
                i++;
            }

            if (i == argument.Length)
            {
                // 末尾的反斜杠：在闭合引号前必须翻倍，否则会把闭合引号转义掉。
                builder.Append('\\', backslashes * 2);
                break;
            }

            if (argument[i] == '"')
            {
                // 引号前的反斜杠翻倍，再把引号写成 \"。
                builder.Append('\\', (backslashes * 2) + 1);
                builder.Append('"');
                continue;
            }

            builder.Append('\\', backslashes);
            builder.Append(argument[i]);
        }

        builder.Append('"');
        return builder.ToString();
    }

    private static readonly char[] NeedsQuoting = { ' ', '\t', '"' };
}
