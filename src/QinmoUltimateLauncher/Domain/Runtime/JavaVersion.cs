using System;
using System.Globalization;

namespace Qul.Domain.Runtime;

/// <summary>
/// Java 版本号。
///
/// 需要同时吃下三代命名：
///   旧式  1.7.0_80 / 1.8.0_503      —— 主版本是第二段，下划线后是 update 号
///   现代  11.0.32 / 17.0.20         —— 主版本就是第一段
///   混合  21.0.12.1 / 25.0.4.1      —— 四段版本号（实测本机 Oracle 24+ 就是这个形态）
///
/// 纯值类型，无 IO，可穷举单测。
/// </summary>
public readonly struct JavaVersion : IEquatable<JavaVersion>, IComparable<JavaVersion>
{
    private JavaVersion(int major, int minor, int patch, int build, int update, string raw)
    {
        Major = major;
        Minor = minor;
        Patch = patch;
        Build = build;
        Update = update;
        Raw = raw;
    }

    public int Major { get; }

    public int Minor { get; }

    public int Patch { get; }

    /// <summary>第四段版本号，例如 21.0.12.<b>1</b> 里的 1。</summary>
    public int Build { get; }

    /// <summary>旧式下划线后的 update 号，例如 1.8.0_<b>503</b> 里的 503。</summary>
    public int Update { get; }

    public string Raw { get; }

    public bool IsValid => Major > 0;

    public static bool TryParse(string? text, out JavaVersion version)
    {
        version = default;

        if (string.IsNullOrWhiteSpace(text))
        {
            return false;
        }

        string token = text!.Trim().Trim('"');
        if (token.Length == 0)
        {
            return false;
        }

        // 截掉构建元数据：1.8.0_503-b01、17.0.20+7-LTS-191、9-ea ……
        int cut = token.IndexOfAny(new[] { '+', '-' });
        if (cut > 0)
        {
            token = token.Substring(0, cut);
        }

        // 下划线之后是 update 号。
        int update = 0;
        int underscore = token.IndexOf('_');
        if (underscore >= 0)
        {
            string tail = token.Substring(underscore + 1);
            token = token.Substring(0, underscore);

            int digits = 0;
            while (digits < tail.Length && tail[digits] >= '0' && tail[digits] <= '9')
            {
                digits++;
            }

            if (digits > 0)
            {
                int.TryParse(tail.Substring(0, digits), NumberStyles.Integer, CultureInfo.InvariantCulture, out update);
            }
        }

        string[] parts = token.Split('.');
        if (parts.Length == 0 || !TryParseInt(parts[0], out int first))
        {
            return false;
        }

        int major;
        int index;

        if (first == 1)
        {
            // 旧式 1.x：主版本在第二段。
            // 只有孤零零一个 "1" 时没有主版本可言——拒绝它，而不是当成 Java 1。
            if (parts.Length < 2 || !TryParseInt(parts[1], out major))
            {
                return false;
            }

            index = 2;
        }
        else
        {
            major = first;
            index = 1;
        }

        if (major <= 0)
        {
            return false;
        }

        int minor = 0;
        int patch = 0;
        int build = 0;

        if (parts.Length > index)
        {
            TryParseInt(parts[index], out minor);
        }

        if (parts.Length > index + 1)
        {
            TryParseInt(parts[index + 1], out patch);
        }

        if (parts.Length > index + 2)
        {
            TryParseInt(parts[index + 2], out build);
        }

        version = new JavaVersion(major, minor, patch, build, update, text.Trim());
        return true;
    }

    /// <summary>
    /// 按 主 / 次 / 修订 / 构建 / update 的顺序比较。
    /// 实际使用中几乎只会比主版本；其余段是为了让同主版本的多个安装有稳定次序。
    /// </summary>
    public int CompareTo(JavaVersion other)
    {
        int result = Major.CompareTo(other.Major);
        if (result != 0) return result;
        result = Minor.CompareTo(other.Minor);
        if (result != 0) return result;
        result = Patch.CompareTo(other.Patch);
        if (result != 0) return result;
        result = Build.CompareTo(other.Build);
        if (result != 0) return result;
        return Update.CompareTo(other.Update);
    }

    public bool Equals(JavaVersion other)
    {
        return CompareTo(other) == 0;
    }

    public override bool Equals(object? obj)
    {
        return obj is JavaVersion other && Equals(other);
    }

    public override int GetHashCode()
    {
        unchecked
        {
            int hash = Major;
            hash = (hash * 397) ^ Minor;
            hash = (hash * 397) ^ Patch;
            hash = (hash * 397) ^ Build;
            hash = (hash * 397) ^ Update;
            return hash;
        }
    }

    public override string ToString()
    {
        return string.IsNullOrEmpty(Raw) ? Major.ToString(CultureInfo.InvariantCulture) : Raw;
    }

    private static bool TryParseInt(string text, out int value)
    {
        return int.TryParse(text, NumberStyles.Integer, CultureInfo.InvariantCulture, out value);
    }
}
