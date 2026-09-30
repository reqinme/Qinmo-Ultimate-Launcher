using System;
using System.Globalization;
using System.Text;

namespace Qul.Infrastructure.Serialization;

/// <summary>
/// 递归下降 JSON 解析器。宽松读取、严格拒绝：接受额外的空白与转义，但拒绝尾部垃圾、
/// 未闭合的字符串、控制字符与超深嵌套（防止畸形输入把栈打爆）。
/// </summary>
internal static class JsonParser
{
    private const int MaxDepth = 256;

    public static JsonValue Parse(string text)
    {
        if (text == null)
        {
            throw new ArgumentNullException(nameof(text));
        }

        State state = new State(text);
        JsonValue value = state.ReadValue(0);
        state.SkipWhitespace();

        if (!state.AtEnd)
        {
            throw new JsonFormatException("unexpected trailing content", state.Position);
        }

        return value;
    }

    private sealed class State
    {
        private readonly string _text;
        private int _pos;

        public State(string text)
        {
            _text = text;
        }

        public int Position => _pos;

        public bool AtEnd => _pos >= _text.Length;

        public void SkipWhitespace()
        {
            while (_pos < _text.Length)
            {
                char c = _text[_pos];
                if (c == ' ' || c == '\t' || c == '\n' || c == '\r')
                {
                    _pos++;
                    continue;
                }

                break;
            }
        }

        public JsonValue ReadValue(int depth)
        {
            if (depth > MaxDepth)
            {
                throw new JsonFormatException("nesting too deep", _pos);
            }

            SkipWhitespace();
            char c = Peek();

            switch (c)
            {
                case '{':
                    return ReadObject(depth);
                case '[':
                    return ReadArray(depth);
                case '"':
                    return new JsonString(ReadString());
                case 't':
                    ReadLiteral("true");
                    return JsonBoolean.True;
                case 'f':
                    ReadLiteral("false");
                    return JsonBoolean.False;
                case 'n':
                    ReadLiteral("null");
                    return JsonNull.Instance;
                default:
                    return ReadNumber();
            }
        }

        private JsonValue ReadObject(int depth)
        {
            Expect('{');
            JsonObject obj = new JsonObject();
            SkipWhitespace();

            if (Peek() == '}')
            {
                _pos++;
                return obj;
            }

            while (true)
            {
                SkipWhitespace();

                if (Peek() != '"')
                {
                    throw new JsonFormatException("expected member name", _pos);
                }

                string key = ReadString();
                SkipWhitespace();
                Expect(':');
                obj.Set(key, ReadValue(depth + 1));
                SkipWhitespace();

                char c = Next();
                if (c == ',')
                {
                    continue;
                }

                if (c == '}')
                {
                    return obj;
                }

                throw new JsonFormatException("expected ',' or '}'", _pos - 1);
            }
        }

        private JsonValue ReadArray(int depth)
        {
            Expect('[');
            JsonArray arr = new JsonArray();
            SkipWhitespace();

            if (Peek() == ']')
            {
                _pos++;
                return arr;
            }

            while (true)
            {
                arr.Add(ReadValue(depth + 1));
                SkipWhitespace();

                char c = Next();
                if (c == ',')
                {
                    continue;
                }

                if (c == ']')
                {
                    return arr;
                }

                throw new JsonFormatException("expected ',' or ']'", _pos - 1);
            }
        }

        private JsonValue ReadNumber()
        {
            int start = _pos;

            while (_pos < _text.Length)
            {
                char c = _text[_pos];
                bool part = (c >= '0' && c <= '9') || c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E';
                if (!part)
                {
                    break;
                }

                _pos++;
            }

            string raw = _text.Substring(start, _pos - start);
            if (raw.Length == 0)
            {
                throw new JsonFormatException("expected value", start);
            }

            if (!double.TryParse(raw, NumberStyles.Float, CultureInfo.InvariantCulture, out double asDouble))
            {
                throw new JsonFormatException("invalid number literal", start);
            }

            bool isInteger = raw.IndexOf('.') < 0 && raw.IndexOf('e') < 0 && raw.IndexOf('E') < 0;
            long asLong = 0;
            if (isInteger)
            {
                long.TryParse(raw, NumberStyles.Integer, CultureInfo.InvariantCulture, out asLong);
            }
            else if (asDouble >= long.MinValue && asDouble <= long.MaxValue)
            {
                asLong = (long)asDouble;
            }

            return JsonNumber.FromLiteral(raw, asDouble, asLong, isInteger);
        }

        private string ReadString()
        {
            Expect('"');
            StringBuilder sb = new StringBuilder();

            while (true)
            {
                if (_pos >= _text.Length)
                {
                    throw new JsonFormatException("unterminated string", _pos);
                }

                char c = _text[_pos++];

                if (c == '"')
                {
                    return sb.ToString();
                }

                if (c == '\\')
                {
                    if (_pos >= _text.Length)
                    {
                        throw new JsonFormatException("unterminated escape sequence", _pos);
                    }

                    char e = _text[_pos++];
                    switch (e)
                    {
                        case '"': sb.Append('"'); break;
                        case '\\': sb.Append('\\'); break;
                        case '/': sb.Append('/'); break;
                        case 'b': sb.Append('\b'); break;
                        case 'f': sb.Append('\f'); break;
                        case 'n': sb.Append('\n'); break;
                        case 'r': sb.Append('\r'); break;
                        case 't': sb.Append('\t'); break;
                        case 'u':
                            if (_pos + 4 > _text.Length)
                            {
                                throw new JsonFormatException("truncated unicode escape", _pos);
                            }

                            int code = 0;
                            for (int i = 0; i < 4; i++)
                            {
                                int digit = HexDigit(_text[_pos + i]);
                                if (digit < 0)
                                {
                                    throw new JsonFormatException("invalid unicode escape", _pos + i);
                                }

                                code = (code << 4) | digit;
                            }

                            _pos += 4;
                            sb.Append((char)code);
                            break;
                        default:
                            throw new JsonFormatException("unknown escape sequence", _pos - 1);
                    }

                    continue;
                }

                if (c < 0x20)
                {
                    throw new JsonFormatException("unescaped control character", _pos - 1);
                }

                sb.Append(c);
            }
        }

        private void ReadLiteral(string literal)
        {
            if (_pos + literal.Length > _text.Length)
            {
                throw new JsonFormatException("truncated literal", _pos);
            }

            for (int i = 0; i < literal.Length; i++)
            {
                if (_text[_pos + i] != literal[i])
                {
                    throw new JsonFormatException("invalid literal", _pos + i);
                }
            }

            _pos += literal.Length;
        }

        private static int HexDigit(char c)
        {
            if (c >= '0' && c <= '9') return c - '0';
            if (c >= 'a' && c <= 'f') return c - 'a' + 10;
            if (c >= 'A' && c <= 'F') return c - 'A' + 10;
            return -1;
        }

        private char Peek()
        {
            if (_pos >= _text.Length)
            {
                throw new JsonFormatException("unexpected end of input", _pos);
            }

            return _text[_pos];
        }

        private char Next()
        {
            char c = Peek();
            _pos++;
            return c;
        }

        private void Expect(char expected)
        {
            char c = Next();
            if (c != expected)
            {
                throw new JsonFormatException("expected '" + expected + "'", _pos - 1);
            }
        }
    }
}

/// <summary>
/// JSON 写出器。
/// 换行固定使用 "\n" 而不是 Environment.NewLine —— 启动计划骨架的逐字节比对要求
/// 跨机器、跨区域设置都产出完全相同的字节。
/// </summary>
internal static class JsonWriter
{
    private const string NewLine = "\n";

    public static string Write(JsonValue value, bool indented)
    {
        StringBuilder sb = new StringBuilder();
        WriteValue(sb, value, indented, 0);
        return sb.ToString();
    }

    private static void WriteValue(StringBuilder sb, JsonValue value, bool indented, int depth)
    {
        switch (value.Kind)
        {
            case JsonKind.Null:
                sb.Append("null");
                break;
            case JsonKind.Boolean:
                sb.Append(((JsonBoolean)value).Value ? "true" : "false");
                break;
            case JsonKind.Number:
                sb.Append(((JsonNumber)value).Raw);
                break;
            case JsonKind.String:
                WriteString(sb, ((JsonString)value).Value);
                break;
            case JsonKind.Array:
                WriteArray(sb, (JsonArray)value, indented, depth);
                break;
            case JsonKind.Object:
                WriteObject(sb, (JsonObject)value, indented, depth);
                break;
            default:
                throw new JsonFormatException("unknown json kind " + value.Kind, 0);
        }
    }

    private static void WriteArray(StringBuilder sb, JsonArray array, bool indented, int depth)
    {
        if (array.Count == 0)
        {
            sb.Append("[]");
            return;
        }

        sb.Append('[');

        for (int i = 0; i < array.Count; i++)
        {
            if (i > 0)
            {
                sb.Append(',');
            }

            if (indented)
            {
                sb.Append(NewLine);
                Indent(sb, depth + 1);
            }

            WriteValue(sb, array[i], indented, depth + 1);
        }

        if (indented)
        {
            sb.Append(NewLine);
            Indent(sb, depth);
        }

        sb.Append(']');
    }

    private static void WriteObject(StringBuilder sb, JsonObject obj, bool indented, int depth)
    {
        if (obj.Count == 0)
        {
            sb.Append("{}");
            return;
        }

        sb.Append('{');

        for (int i = 0; i < obj.Keys.Count; i++)
        {
            if (i > 0)
            {
                sb.Append(',');
            }

            if (indented)
            {
                sb.Append(NewLine);
                Indent(sb, depth + 1);
            }

            WriteString(sb, obj.Keys[i]);
            sb.Append(':');
            if (indented)
            {
                sb.Append(' ');
            }

            JsonValue child = obj[obj.Keys[i]] ?? JsonNull.Instance;
            WriteValue(sb, child, indented, depth + 1);
        }

        if (indented)
        {
            sb.Append(NewLine);
            Indent(sb, depth);
        }

        sb.Append('}');
    }

    private static void Indent(StringBuilder sb, int depth)
    {
        sb.Append(' ', depth * 2);
    }

    private static void WriteString(StringBuilder sb, string value)
    {
        sb.Append('"');

        for (int i = 0; i < value.Length; i++)
        {
            char c = value[i];
            switch (c)
            {
                case '"': sb.Append("\\\""); break;
                case '\\': sb.Append("\\\\"); break;
                case '\b': sb.Append("\\b"); break;
                case '\f': sb.Append("\\f"); break;
                case '\n': sb.Append("\\n"); break;
                case '\r': sb.Append("\\r"); break;
                case '\t': sb.Append("\\t"); break;
                default:
                    if (c < 0x20)
                    {
                        sb.Append("\\u");
                        sb.Append(((int)c).ToString("x4", CultureInfo.InvariantCulture));
                    }
                    else
                    {
                        sb.Append(c);
                    }

                    break;
            }
        }

        sb.Append('"');
    }
}
