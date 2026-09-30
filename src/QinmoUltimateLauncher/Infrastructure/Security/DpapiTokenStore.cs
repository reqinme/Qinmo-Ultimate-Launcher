using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Security.Cryptography;
using System.Text;
using Qul.Application.Ports;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;
using Qul.Domain.Identity;
using Qul.Infrastructure.Diagnostics;
using Qul.Infrastructure.Serialization;

namespace Qul.Infrastructure.Security;

/// <summary>
/// 基于系统凭据保护（DPAPI）的会话存储。
///
/// **落盘一律是密文。** 明文令牌只在内存里存在，写盘前就被 <see cref="ProtectedData"/> 加密；
/// 这一点由测试直接扫描文件字节来守住，而不是靠实现自觉。
///
/// 作用域用 <see cref="DataProtectionScope.CurrentUser"/>：密文与当前 Windows 账户绑定。
/// 副作用是把 data 目录整体拷到另一台机器或另一个账户后解不开——
/// 那是预期行为，不是缺陷；解不开时按"当作没登录过"处理。
/// </summary>
public sealed class DpapiTokenStore : ITokenStore
{
    private const string FileExtension = ".bin";

    private readonly string _directory;
    private readonly SessionLog _log;

    public DpapiTokenStore(string secretsDirectory, SessionLog? log = null)
    {
        if (string.IsNullOrWhiteSpace(secretsDirectory))
        {
            throw new ArgumentException("secrets directory is required", nameof(secretsDirectory));
        }

        _directory = secretsDirectory!;
        _log = log ?? SessionLog.Null;
    }

    public string Directory => _directory;

    public void Save(string accountKey, AuthSession session)
    {
        if (string.IsNullOrWhiteSpace(accountKey))
        {
            throw new ArgumentException("account key is required", nameof(accountKey));
        }

        if (session == null)
        {
            throw new ArgumentNullException(nameof(session));
        }

        byte[] plain = Encoding.UTF8.GetBytes(ToJson(accountKey, session));

        try
        {
            System.IO.Directory.CreateDirectory(_directory);

            byte[] cipher = ProtectedData.Protect(plain, null, DataProtectionScope.CurrentUser);

            string path = PathFor(accountKey);
            string temp = path + ".tmp";

            File.WriteAllBytes(temp, cipher);

            if (File.Exists(path))
            {
                File.Delete(path);
            }

            File.Move(temp, path);
        }
        catch (CryptographicException ex)
        {
            throw LauncherException.Wrap(ErrorCode.AuthTokenRefreshFailed, ex);
        }
        catch (IOException ex)
        {
            throw LauncherException.Wrap(ErrorCode.IoDataRootNotWritable, ex);
        }
        catch (UnauthorizedAccessException ex)
        {
            throw LauncherException.Wrap(ErrorCode.IoDataRootNotWritable, ex);
        }
        finally
        {
            // 尽力而为：托管内存无法保证擦除，但至少不留长期引用。
            Array.Clear(plain, 0, plain.Length);
        }
    }

    public AuthSession? Load(string accountKey)
    {
        if (string.IsNullOrWhiteSpace(accountKey))
        {
            return null;
        }

        string path = PathFor(accountKey);
        if (!File.Exists(path))
        {
            return null;
        }

        return TryDecrypt(path, out AuthSession? session, out string _) ? session : null;
    }

    public void Delete(string accountKey)
    {
        if (string.IsNullOrWhiteSpace(accountKey))
        {
            return;
        }

        SafeDelete(PathFor(accountKey));
    }

    /// <summary>列出可解密的账户键。解不开的文件会被跳过并清除，不会让整个列表失败。</summary>
    public IReadOnlyList<string> ListAccounts()
    {
        List<string> accounts = new List<string>();

        if (!System.IO.Directory.Exists(_directory))
        {
            return accounts;
        }

        string[] files;
        try
        {
            files = System.IO.Directory.GetFiles(_directory, "*" + FileExtension);
        }
        catch (IOException)
        {
            return accounts;
        }
        catch (UnauthorizedAccessException)
        {
            return accounts;
        }

        for (int i = 0; i < files.Length; i++)
        {
            if (TryDecrypt(files[i], out AuthSession? _, out string accountKey) && accountKey.Length > 0)
            {
                accounts.Add(accountKey);
            }
        }

        return accounts;
    }

    private bool TryDecrypt(string path, out AuthSession? session, out string accountKey)
    {
        session = null;
        accountKey = string.Empty;

        try
        {
            byte[] cipher = File.ReadAllBytes(path);
            byte[] plain = ProtectedData.Unprotect(cipher, null, DataProtectionScope.CurrentUser);

            try
            {
                JsonObject root = JsonValue.Parse(Encoding.UTF8.GetString(plain)).RequireObject();
                accountKey = root.GetString("accountKey") ?? string.Empty;
                session = FromJson(root);
                return true;
            }
            finally
            {
                Array.Clear(plain, 0, plain.Length);
            }
        }
        catch (CryptographicException)
        {
            // 换了 Windows 账户、漫游配置变了、或者密文被改坏。
            // 正确行为是"当作没有登录过，请重新登录"——不是崩溃，也不是拿一个空身份继续。
            _log.Warn("auth", "stored credential cannot be decrypted; discarding", ErrorCode.AuthTokenRefreshFailed);
            SafeDelete(path);
            return false;
        }
        catch (Exception ex) when (ex is IOException || ex is JsonFormatException || ex is ArgumentException)
        {
            _log.Warn("auth", "stored credential is unreadable; discarding", ErrorCode.AuthTokenRefreshFailed);
            SafeDelete(path);
            return false;
        }
    }

    /// <summary>文件名取账户键的摘要，避免账户名里的字符影响文件系统。</summary>
    private string PathFor(string accountKey)
    {
        byte[] bytes = Encoding.UTF8.GetBytes(accountKey);

        using (SHA1 sha1 = SHA1.Create())
        {
            byte[] hash = sha1.ComputeHash(bytes);
            StringBuilder builder = new StringBuilder(hash.Length * 2);

            for (int i = 0; i < hash.Length; i++)
            {
                builder.Append(hash[i].ToString("x2", CultureInfo.InvariantCulture));
            }

            return Path.Combine(_directory, builder.ToString() + FileExtension);
        }
    }

    private static void SafeDelete(string path)
    {
        try
        {
            if (File.Exists(path))
            {
                File.Delete(path);
            }
        }
        catch (IOException)
        {
        }
        catch (UnauthorizedAccessException)
        {
        }
    }

    private static string ToJson(string accountKey, AuthSession session)
    {
        JsonObject root = new JsonObject()
            .Set("formatVersion", 1)
            .Set("accountKey", accountKey)
            .Set("source", session.Source.ToString())
            .Set("userName", session.UserName)
            .Set("uuid", session.Uuid)
            .Set("accessToken", session.AccessToken)
            .Set("refreshToken", session.RefreshToken)
            .Set("expiresAt", session.ExpiresAt.ToUniversalTime().ToString("o", CultureInfo.InvariantCulture))
            .Set("xboxUserId", session.XboxUserId)
            .Set("userType", session.UserType);

        return root.ToJson(false);
    }

    private static AuthSession FromJson(JsonObject root)
    {
        return new AuthSession
        {
            Source = ParseSource(root.GetString("source")),
            UserName = root.GetString("userName") ?? string.Empty,
            Uuid = root.GetString("uuid") ?? string.Empty,
            AccessToken = root.GetString("accessToken") ?? string.Empty,
            RefreshToken = root.GetString("refreshToken") ?? string.Empty,
            ExpiresAt = ParseTime(root.GetString("expiresAt")),
            XboxUserId = root.GetString("xboxUserId"),
            UserType = root.GetString("userType") ?? "msa",
        };
    }

    private static DateTimeOffset ParseTime(string? text)
    {
        return DateTimeOffset.TryParse(
            text,
            CultureInfo.InvariantCulture,
            DateTimeStyles.RoundtripKind,
            out DateTimeOffset value)
            ? value
            : DateTimeOffset.MinValue;
    }

    private static IdentitySource ParseSource(string? text)
    {
        if (string.Equals(text, "Offline", StringComparison.OrdinalIgnoreCase))
        {
            return IdentitySource.Offline;
        }

        if (string.Equals(text, "ThirdParty", StringComparison.OrdinalIgnoreCase))
        {
            return IdentitySource.ThirdParty;
        }

        return IdentitySource.Microsoft;
    }
}
