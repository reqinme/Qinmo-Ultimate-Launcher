using System.Collections.Generic;
using Qul.Domain.Identity;

namespace Qul.Application.Ports;

/// <summary>
/// 会话凭据的本地存储。
///
/// **契约要求：落盘必须是密文。** 实现不得以任何形式在磁盘上留下明文令牌——
/// 这一点由测试直接扫描文件字节来守住，而不是靠实现自觉。
///
/// 另外一条同样重要：**读不出内容时不得抛异常**。
/// 换了 Windows 账户、漫游配置或密文被改坏，都会让解密失败；
/// 那时候正确行为是"当作没有登录过，请重新登录"，而不是让启动器崩掉。
/// </summary>
public interface ITokenStore
{
    /// <summary>保存会话。会覆盖同一账户的旧记录。</summary>
    void Save(string accountKey, AuthSession session);

    /// <summary>读取会话；不存在或无法解密时返回 null，并清除无法使用的残留。</summary>
    AuthSession? Load(string accountKey);

    void Delete(string accountKey);

    IReadOnlyList<string> ListAccounts();
}
