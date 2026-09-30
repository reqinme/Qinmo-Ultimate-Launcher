using System.Collections.Generic;
using System.Text;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Infrastructure.Serialization;

namespace Qul.Tests.Infrastructure;

[TestClass]
public sealed class JsonTests
{
    [TestMethod]
    public void Parse_PreservesMemberOrder()
    {
        JsonValue value = JsonValue.Parse("{\"z\":1,\"a\":2,\"m\":3}");

        JsonObject obj = value.RequireObject();

        CollectionAssert.AreEqual(new[] { "z", "a", "m" }, new List<string>(obj.Keys));
        Assert.AreEqual("{\"z\":1,\"a\":2,\"m\":3}", value.ToJson());
    }

    [TestMethod]
    public void Parse_HandlesEscapesAndUnicode()
    {
        JsonValue value = JsonValue.Parse("{\"text\":\"line\\nbreak \\\"quoted\\\" \\u4e2d\\u6587 \\\\ end\"}");

        Assert.AreEqual("line\nbreak \"quoted\" 中文 \\ end", value.RequireObject().GetString("text"));
    }

    [TestMethod]
    public void Parse_RejectsTrailingContent()
    {
        Assert.ThrowsException<JsonFormatException>(() => JsonValue.Parse("{} extra"));
    }

    [TestMethod]
    public void Parse_RejectsEmptyInput()
    {
        Assert.ThrowsException<JsonFormatException>(() => JsonValue.Parse("   "));
    }

    [TestMethod]
    public void Parse_RejectsUnterminatedString()
    {
        Assert.ThrowsException<JsonFormatException>(() => JsonValue.Parse("{\"a\":\"unterminated"));
    }

    [TestMethod]
    public void Parse_RejectsUnescapedControlCharacter()
    {
        Assert.ThrowsException<JsonFormatException>(() => JsonValue.Parse("{\"a\":\"bad\u0001char\"}"));
    }

    [TestMethod]
    public void Parse_RejectsExcessiveNesting()
    {
        // 畸形输入不得把栈打爆：深度上限必须真的生效。
        string deep = new string('[', 400) + new string(']', 400);

        Assert.ThrowsException<JsonFormatException>(() => JsonValue.Parse(deep));
    }

    [TestMethod]
    public void RoundTrip_PreservesNumberLiteralsExactly()
    {
        // 启动计划骨架要逐字节比对，因此数字必须原样往返，不能被格式化改写。
        const string source = "{\"a\":1,\"b\":1.5,\"c\":-2,\"d\":1e3,\"e\":0.10}";

        Assert.AreEqual(source, JsonValue.Parse(source).ToJson());
    }

    [TestMethod]
    public void Writer_IndentedOutputUsesLineFeedOnly()
    {
        JsonObject obj = new JsonObject().Set("a", 1).Set("b", "x");

        string text = obj.ToJson(indented: true);

        // 跨机器字节一致要求：绝不使用 Environment.NewLine。
        Assert.IsFalse(text.Contains("\r"), "缩进输出不得包含回车符");
        StringAssert.Contains(text, "\n");
        StringAssert.Contains(text, "  \"a\": 1");
    }

    [TestMethod]
    public void Writer_EscapesControlCharactersAndQuotes()
    {
        JsonObject obj = new JsonObject().Set("k", "a\"b\\c\td\ne");

        string text = obj.ToJson();

        Assert.AreEqual("{\"k\":\"a\\\"b\\\\c\\td\\ne\"}", text);
        Assert.AreEqual("a\"b\\c\td\ne", JsonValue.Parse(text).RequireObject().GetString("k"));
    }

    [TestMethod]
    public void Accessors_FallBackInsteadOfThrowing()
    {
        JsonObject obj = JsonValue.Parse("{\"n\":null,\"s\":\"x\",\"i\":7,\"b\":true}").RequireObject();

        // 元数据跨年代差异极大，"读不到就用默认值"是硬要求，不能靠抛异常表达缺字段。
        Assert.IsNull(obj.GetString("missing"));
        Assert.AreEqual("fallback", obj.GetString("missing", "fallback"));
        Assert.IsNull(obj.GetString("n"), "null 应被视为缺失");
        Assert.AreEqual("x", obj.GetString("s"));
        Assert.AreEqual(7, obj.GetInt("i"));
        Assert.AreEqual(42, obj.GetInt("missing", 42));
        Assert.IsTrue(obj.GetBoolean("b"));
        Assert.IsFalse(obj.GetBoolean("missing"));
        Assert.IsNull(obj.GetArray("s"), "类型不符应返回 null 而不是抛异常");
    }

    [TestMethod]
    public void Parse_HandlesNestedStructures()
    {
        const string source = "{\"libs\":[{\"name\":\"a\"},{\"name\":\"b\"}],\"rules\":[{\"action\":\"allow\"}]}";

        JsonObject root = JsonValue.Parse(source).RequireObject();
        JsonArray libs = root.GetArray("libs")!;

        Assert.AreEqual(2, libs.Count);
        Assert.AreEqual("b", libs[1].RequireObject().GetString("name"));
        Assert.AreEqual(source, root.ToJson());
    }
}
