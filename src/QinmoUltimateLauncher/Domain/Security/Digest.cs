using System;

namespace Qul.Domain.Security;

/// <summary>
/// 十六进制 SHA-1 摘要。
/// 只做格式校验与大小写无关比较，不做任何 IO——摘要的计算属于基础设施。
/// </summary>
public readonly struct Sha1Hex : IEquatable<Sha1Hex>
{
    public const int HexLength = 40;

    private readonly string? _value;

    private Sha1Hex(string value)
    {
        _value = value;
    }

    /// <summary>归一化为小写十六进制。无效时为 null。</summary>
    public string? Value => _value;

    public bool IsValid => _value != null && _value.Length == HexLength;

    public static bool TryParse(string? text, out Sha1Hex digest)
    {
        digest = default;

        if (text == null)
        {
            return false;
        }

        string trimmed = text.Trim();
        if (trimmed.Length != HexLength)
        {
            return false;
        }

        for (int i = 0; i < trimmed.Length; i++)
        {
            char c = trimmed[i];
            bool isHex = (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F');
            if (!isHex)
            {
                return false;
            }
        }

        // 归一化为小写，比较时就不必再关心大小写。
        digest = new Sha1Hex(trimmed.ToLowerInvariant());
        return true;
    }

    public bool Equals(Sha1Hex other)
    {
        return string.Equals(_value, other._value, StringComparison.Ordinal);
    }

    public override bool Equals(object? obj)
    {
        return obj is Sha1Hex other && Equals(other);
    }

    public override int GetHashCode()
    {
        return _value == null ? 0 : _value.GetHashCode();
    }

    public override string ToString()
    {
        return _value ?? string.Empty;
    }
}
