using System;
using System.Windows;
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
                return new Duration(TimeSpan.FromMilliseconds(120));
            case MotionSpeed.Slow:
                return new Duration(TimeSpan.FromMilliseconds(240));
            default:
                return new Duration(TimeSpan.FromMilliseconds(180));
        }
    }
}
