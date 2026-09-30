namespace Qul.Domain.Launch;

/// <summary>
/// 启动参数所依赖的特性开关。
///
/// 实测结论（来自 1.16.5 与 26.3 的真实元数据）：一部分参数条目不是被 os 规则门控，
/// 而是被 <c>features</c> 门控——
///   --demo                     需要 is_demo_user = true
///   --width / --height         需要 has_custom_resolution = true
///   --quickPlayPath            需要 has_quick_plays_support = true
///   --quickPlaySingleplayer    需要 is_quick_play_singleplayer = true
///   --quickPlayMultiplayer     需要 is_quick_play_multiplayer = true
///   --quickPlayRealms          需要 is_quick_play_realms = true
///
/// **不声明 feature，这些参数根本进不了启动计划**——它们不是"取值为空所以被省略"，
/// 而是压根没通过规则求值。搞混这两件事会让人对着一个永远不出现的 --width 查半天。
/// </summary>
public static class LaunchFeatures
{
    public const string IsDemoUser = "is_demo_user";
    public const string HasCustomResolution = "has_custom_resolution";
    public const string HasQuickPlaysSupport = "has_quick_plays_support";
    public const string IsQuickPlaySingleplayer = "is_quick_play_singleplayer";
    public const string IsQuickPlayMultiplayer = "is_quick_play_multiplayer";
    public const string IsQuickPlayRealms = "is_quick_play_realms";
}
