using System;
using System.Windows;
using System.Windows.Media.Animation;
using System.Windows.Markup;

namespace Qul.Presentation;

public enum MotionSpeed
{
    Fast = 0,
    Normal = 1,
    Slow = 2,
}

/// <summary>
/// 动效时长的**唯一来源**：<c>Duration="{qul:Motion Fast}"</c>
///
/// 为什么不用资源字典里的 Duration 令牌：
/// <see cref="System.Windows.Media.Animation.Storyboard"/> 的 Duration 在**解析时就被烘焙进模板**，
/// 之后再改资源字典对它没有任何影响。也就是说"运行期换令牌来关掉动画"这条路根本走不通。
///
/// 标记扩展在解析期就决定了取值，所以它是可靠的。
/// 系统关闭了动态效果（设置 → 辅助功能 → 视觉效果 → 动画效果）时时长归零。
/// 这不是可选项：前庭功能敏感的用户会因此不适。
/// </summary>
public sealed class MotionExtension : MarkupExtension
{
    public MotionExtension()
    {
    }

    public MotionExtension(MotionSpeed speed)
    {
        Speed = speed;
    }

    public MotionSpeed Speed { get; set; } = MotionSpeed.Normal;

    public override object ProvideValue(IServiceProvider serviceProvider)
    {
        if (!SystemParameters.ClientAreaAnimation)
        {
            return new Duration(TimeSpan.Zero);
        }

        switch (Speed)
        {
            case MotionSpeed.Fast:
                // **150 而不是 120。** P6 的要求与 P6.5 规范 L20 都写的是"150–250 ms"，
                // 而这里原本是 120——低于下限。规范 L362 那句"120/180/240"是笔误，
                // 它把自己 L20 的区间写错了，两处互相矛盾。
                return new Duration(TimeSpan.FromMilliseconds(150));
            case MotionSpeed.Slow:
                return new Duration(TimeSpan.FromMilliseconds(240));
            default:
                return new Duration(TimeSpan.FromMilliseconds(180));
        }
    }
}
/// <summary>
/// 重复次数的标记扩展：<c>RepeatBehavior="{qul:MotionRepeat}"</c>。
///
/// **存在的理由**：不确定进度条需要一段持续脉动，而
/// <see cref="MotionExtension"/> 在"减少动态效果"打开时返回 0 时长——
/// 把 0 时长喂给 <c>RepeatBehavior="Forever"</c> 会让合成器全速空转，
/// 比动画本身还糟。
///
/// 所以这里不返回时长，而是返回**重复次数**：动画关掉时只播一次（等于不动），
/// 打开时才是 Forever。这样"持续动画"与"尊重系统设置"两件事不再互相打架。
/// </summary>
public sealed class MotionRepeatExtension : MarkupExtension
{
    public override object ProvideValue(IServiceProvider serviceProvider)
    {
        return SystemParameters.ClientAreaAnimation
            ? RepeatBehavior.Forever
            : new RepeatBehavior(1);
    }
}
