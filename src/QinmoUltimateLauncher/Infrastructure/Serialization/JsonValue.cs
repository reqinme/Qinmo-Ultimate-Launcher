using System;
using System.Collections.Generic;

namespace Qul.Infrastructure.Serialization;

public enum JsonKind
{
    Null,
    Boolean,
    Number,
    String,
    Array,
    Object,
}

/// <summary>JSON 文本非法。由调用方决定映射成哪个错误码（配置用 CFG，元数据用 META）。</summary>
public sealed class JsonFormatException : Exception
{
    public int Position { get; }

    public JsonFormatException(string message, int position)
        : base(message)
    {
        Position = position;
    }
}

/// <summary>
/// 手写 JSON DOM。
/// 选择手写而非引入库或 System.Web.Extensions，理由有三：
/// 1) 冷启动热路径上不多加载一个框架程序集（S2 已显示首启余量最紧）；
/// 2) 成员顺序必须可保证——启动计划骨架比对与元数据往返都依赖它；
/// 3) 无长度上限，且与"每引入一个包都要写明理由与体积代价"的依赖策略一致。
/// </summary>
public abstract class JsonValue
{
    public abstract JsonKind Kind { get; }

    public bool IsNull => Kind == JsonKind.Null;

    public bool IsObject => Kind == JsonKind.Object;

    public bool IsArray => Kind == JsonKind.Array;

    public bool IsString => Kind == JsonKind.String;

    public bool IsNumber => Kind == JsonKind.Number;

    public bool IsBoolean => Kind == JsonKind.Boolean;

    public static JsonValue Parse(string text)
    {
        return JsonParser.Parse(text);
    }

    public static JsonValue FromString(string? value)
    {
        return value is null ? JsonNull.Instance : new JsonString(value);
    }

    public static JsonValue FromNumber(long value)
    {
        return JsonNumber.FromLong(value);
    }

    public static JsonValue FromBoolean(bool value)
    {
        return value ? JsonBoolean.True : JsonBoolean.False;
    }

    public static JsonObject NewObject()
    {
        return new JsonObject();
    }

    public static JsonArray NewArray()
    {
        return new JsonArray();
    }

    public JsonObject RequireObject()
    {
        if (this is JsonObject obj)
        {
            return obj;
        }

        throw new JsonFormatException("expected object but found " + Kind, 0);
    }

    public JsonArray RequireArray()
    {
        if (this is JsonArray arr)
        {
            return arr;
        }

        throw new JsonFormatException("expected array but found " + Kind, 0);
    }

    public string RequireString()
    {
        if (this is JsonString str)
        {
            return str.Value;
        }

        throw new JsonFormatException("expected string but found " + Kind, 0);
    }

    public string ToJson(bool indented = false)
    {
        return JsonWriter.Write(this, indented);
    }

    public override string ToString()
    {
        return ToJson(false);
    }
}

public sealed class JsonNull : JsonValue
{
    public static readonly JsonNull Instance = new JsonNull();

    private JsonNull()
    {
    }

    public override JsonKind Kind => JsonKind.Null;
}

public sealed class JsonBoolean : JsonValue
{
    public static readonly JsonBoolean True = new JsonBoolean(true);
    public static readonly JsonBoolean False = new JsonBoolean(false);

    private JsonBoolean(bool value)
    {
        Value = value;
    }

    public bool Value { get; }

    public override JsonKind Kind => JsonKind.Boolean;
}

public sealed class JsonNumber : JsonValue
{
    private JsonNumber(string raw, double asDouble, long asLong, bool isInteger)
    {
        Raw = raw;
        AsDouble = asDouble;
        AsLong = asLong;
        IsInteger = isInteger;
    }

    /// <summary>原始字面量，用于无损往返。</summary>
    public string Raw { get; }

    public double AsDouble { get; }

    public long AsLong { get; }

    public bool IsInteger { get; }

    public override JsonKind Kind => JsonKind.Number;

    public static JsonNumber FromLong(long value)
    {
        return new JsonNumber(value.ToString(System.Globalization.CultureInfo.InvariantCulture), value, value, true);
    }

    public static JsonNumber FromDouble(double value)
    {
        return new JsonNumber(
            value.ToString("R", System.Globalization.CultureInfo.InvariantCulture),
            value,
            (long)value,
            false);
    }

    internal static JsonNumber FromLiteral(string raw, double asDouble, long asLong, bool isInteger)
    {
        return new JsonNumber(raw, asDouble, asLong, isInteger);
    }
}

public sealed class JsonString : JsonValue
{
    public JsonString(string value)
    {
        Value = value ?? throw new ArgumentNullException(nameof(value));
    }

    public string Value { get; }

    public override JsonKind Kind => JsonKind.String;
}

public sealed class JsonArray : JsonValue
{
    private readonly List<JsonValue> _items = new List<JsonValue>();

    public override JsonKind Kind => JsonKind.Array;

    public IReadOnlyList<JsonValue> Items => _items;

    public int Count => _items.Count;

    public JsonValue this[int index] => _items[index];

    public JsonArray Add(JsonValue value)
    {
        _items.Add(value ?? JsonNull.Instance);
        return this;
    }

    public JsonArray Add(string? value)
    {
        return Add(FromString(value));
    }

    public JsonArray Add(long value)
    {
        return Add(FromNumber(value));
    }

    public IEnumerable<JsonValue> Enumerate()
    {
        return _items;
    }
}

/// <summary>保序 JSON 对象。成员顺序在往返后保持不变。</summary>
public sealed class JsonObject : JsonValue
{
    private readonly List<string> _order = new List<string>();
    private readonly Dictionary<string, JsonValue> _members = new Dictionary<string, JsonValue>(StringComparer.Ordinal);

    public override JsonKind Kind => JsonKind.Object;

    /// <summary>按插入顺序返回成员名。</summary>
    public IReadOnlyList<string> Keys => _order;

    public int Count => _order.Count;

    public JsonValue? this[string key]
    {
        get
        {
            return _members.TryGetValue(key, out JsonValue? value) ? value : null;
        }
    }

    public bool ContainsKey(string key)
    {
        return _members.ContainsKey(key);
    }

    public JsonObject Set(string key, JsonValue? value)
    {
        if (key == null)
        {
            throw new ArgumentNullException(nameof(key));
        }

        if (!_members.ContainsKey(key))
        {
            _order.Add(key);
        }

        _members[key] = value ?? JsonNull.Instance;
        return this;
    }

    public JsonObject Set(string key, string? value)
    {
        return Set(key, FromString(value));
    }

    public JsonObject Set(string key, long value)
    {
        return Set(key, FromNumber(value));
    }

    /// <summary>可空整数专用入口。不用重载，否则 <c>Set(key, someInt)</c> 会在 long 与 int? 之间产生歧义。</summary>
    public JsonObject SetNullableInt(string key, int? value)
    {
        return value.HasValue ? Set(key, FromNumber(value.Value)) : Set(key, JsonNull.Instance);
    }

    public JsonObject Set(string key, bool value)
    {
        return Set(key, FromBoolean(value));
    }

    public bool TryGet(string key, out JsonValue value)
    {
        if (_members.TryGetValue(key, out JsonValue? found))
        {
            value = found;
            return true;
        }

        value = JsonNull.Instance;
        return false;
    }

    /// <summary>读取字符串；缺失、为 null 或类型不符时返回 fallback。</summary>
    public string? GetString(string key, string? fallback = null)
    {
        return _members.TryGetValue(key, out JsonValue? value) && value is JsonString str ? str.Value : fallback;
    }

    /// <summary>读取整数；缺失或类型不符时返回 fallback。兼容整型与浮点写法。</summary>
    public int? GetInt(string key, int? fallback = null)
    {
        if (_members.TryGetValue(key, out JsonValue? value) && value is JsonNumber number)
        {
            return (int)number.AsLong;
        }

        return fallback;
    }

    public bool GetBoolean(string key, bool fallback = false)
    {
        return _members.TryGetValue(key, out JsonValue? value) && value is JsonBoolean b ? b.Value : fallback;
    }

    /// <summary>读取 64 位整数。资源体积会超出 int 范围（assetIndex.totalSize 可达数 GB），必须用 long。</summary>
    public long? GetLong(string key, long? fallback = null)
    {
        if (_members.TryGetValue(key, out JsonValue? value) && value is JsonNumber number)
        {
            return number.IsInteger ? number.AsLong : (long)number.AsDouble;
        }

        return fallback;
    }

    public JsonArray? GetArray(string key)
    {
        return _members.TryGetValue(key, out JsonValue? value) && value is JsonArray arr ? arr : null;
    }

    public JsonObject? GetObject(string key)
    {
        return _members.TryGetValue(key, out JsonValue? value) && value is JsonObject obj ? obj : null;
    }
}
