namespace Qul.Application.Update;

/// <summary>
/// 发布源地址。
///
/// **发布时必须填这里**（或用构建参数覆盖）。留空时界面上的「检查更新」是禁用的，
/// 并明确告诉用户"尚未配置发布源"——而不是让它去请求一个不存在的地址然后报一个看不懂的错。
/// </summary>
public static class UpdateEndpoints
{
    /// <summary>发布清单地址。留空表示当前构建没有配置发布源。</summary>
    public const string ReleaseManifestUrl = "";

    public static bool IsConfigured => !string.IsNullOrWhiteSpace(ReleaseManifestUrl);

    /// <summary>没有配置发布源时给用户看的说明。</summary>
    public const string NotConfiguredHint = "当前构建尚未配置发布源，无法检查更新。";
}
