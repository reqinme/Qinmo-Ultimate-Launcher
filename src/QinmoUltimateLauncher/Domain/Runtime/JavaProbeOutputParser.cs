using System;
using System.Collections.Generic;
using System.Globalization;

namespace Qul.Domain.Runtime;

/// <summary>
/// 解析 java.exe 的输出，得到版本与架构。
///
/// 纯字符串逻辑，因此可以用真实抓取的输出做穷举回归。
/// 支持两种输出形态：
///   1) <c>-XshowSettings:properties -version</c> —— 权威形态，直接给出 java.version / os.arch / sun.arch.data.model
///   2) <c>-version</c> —— 回落形态，从 <c>java version "1.8.0_503"</c> 这类行里取版本号
///
/// 实测要点：这两条命令的输出**全部走 stderr**（stdout 为 0 字节），调用方必须同时收两个流。
/// </summary>
public static class JavaProbeOutputParser
{
    private const string KeyVersion = "java.version";
    private const string KeyOsArch = "os.arch";
    private const string KeyDataModel = "sun.arch.data.model";
    private const string KeyVendor = "java.vendor";
    private const string KeyVmName = "java.vm.name";

    public static bool TryParse(string? stdout, string? stderr, out JavaProbeResult result)
    {
        result = new JavaProbeResult();

        string combined = (stdout ?? string.Empty) + "\n" + (stderr ?? string.Empty);
        if (combined.Trim().Length == 0)
        {
            return false;
        }

        string? versionText = null;
        string? osArch = null;
        string? vendor = null;
        string? vmName = null;
        int dataModel = 0;

        string[] lines = combined.Split('\n');

        for (int i = 0; i < lines.Length; i++)
        {
            string line = lines[i].TrimEnd('\r');
            if (!TryReadKeyValue(line, out string key, out string value))
            {
                continue;
            }

            switch (key)
            {
                case KeyVersion:
                    versionText = value;
                    break;
                case KeyOsArch:
                    osArch = value;
                    break;
                case KeyDataModel:
                    int.TryParse(value, NumberStyles.Integer, CultureInfo.InvariantCulture, out dataModel);
                    break;
                case KeyVendor:
                    vendor = value;
                    break;
                case KeyVmName:
                    vmName = value;
                    break;
            }
        }

        // 回落：属性输出不可用时，从 "java version \"1.8.0_503\"" 这类行里取。
        if (string.IsNullOrEmpty(versionText))
        {
            for (int i = 0; i < lines.Length; i++)
            {
                if (TryReadQuotedVersion(lines[i], out string quoted))
                {
                    versionText = quoted;
                    break;
                }
            }
        }

        if (!JavaVersion.TryParse(versionText, out JavaVersion version))
        {
            return false;
        }

        result.Version = version;
        result.OsArch = JavaRuntimeCandidate.NormalizeArch(osArch);
        result.Vendor = vendor;
        result.VmName = vmName;
        result.Bitness = ResolveBitness(dataModel, vmName);

        if (result.Bitness == 0)
        {
            // 纯 -version 输出里没有属性键值对，captured vmName 会是 null；
            // 退一步从整段文本里找 "64-Bit Server VM" 这类字样。
            result.Bitness = ResolveBitness(0, combined);
        }

        return result.IsUsable;
    }

    /// <summary>
    /// 位宽优先取 <c>sun.arch.data.model</c>（直接给出 32/64），
    /// 拿不到再从 vm name 里的 "64-Bit" / "32-Bit" 推断。
    /// 两个都拿不到就返回 0，表示未知——绝不用架构名去猜位宽。
    /// </summary>
    public static int ResolveBitness(int dataModel, string? vmName)
    {
        if (dataModel == 32 || dataModel == 64)
        {
            return dataModel;
        }

        if (!string.IsNullOrEmpty(vmName))
        {
            if (vmName!.IndexOf("64-Bit", StringComparison.OrdinalIgnoreCase) >= 0)
            {
                return 64;
            }

            if (vmName!.IndexOf("32-Bit", StringComparison.OrdinalIgnoreCase) >= 0)
            {
                return 32;
            }
        }

        return 0;
    }

    /// <summary>读一行 <c>key = value</c>。用第一个等号切分，因为值里本身可能含等号。</summary>
    private static bool TryReadKeyValue(string line, out string key, out string value)
    {
        key = string.Empty;
        value = string.Empty;

        int equals = line.IndexOf('=');
        if (equals <= 0)
        {
            return false;
        }

        key = line.Substring(0, equals).Trim();
        value = line.Substring(equals + 1).Trim();

        return key.Length > 0;
    }

    private static bool TryReadQuotedVersion(string line, out string version)
    {
        version = string.Empty;

        const string Marker = "version \"";
        int at = line.IndexOf(Marker, StringComparison.OrdinalIgnoreCase);
        if (at < 0)
        {
            return false;
        }

        int start = at + Marker.Length;
        int end = line.IndexOf('"', start);
        if (end <= start)
        {
            return false;
        }

        version = line.Substring(start, end - start);
        return version.Length > 0;
    }
}
