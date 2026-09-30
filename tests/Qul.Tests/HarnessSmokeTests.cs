using System.Linq;
using System.Reflection;
using System.Runtime.Versioning;
using Microsoft.VisualStudio.TestTools.UnitTesting;

namespace Qul.Tests
{
    /// <summary>
    /// P0 夹具自检：验证测试链路本身可用，且产品程序集确实按 net48 产出。
    /// 这不是产品功能测试；P1 起由真实的版本元数据解析用例接管主要覆盖。
    /// </summary>
    [TestClass]
    public sealed class HarnessSmokeTests
    {
        [TestMethod]
        public void ProductAssembly_IsLoadable()
        {
            Assembly asm = Assembly.Load("QinmoUltimateLauncher");

            Assert.IsNotNull(asm);
            Assert.AreEqual("QinmoUltimateLauncher", asm.GetName().Name);
        }

        [TestMethod]
        public void ProductAssembly_TargetsNetFramework48()
        {
            Assembly asm = Assembly.Load("QinmoUltimateLauncher");

            TargetFrameworkAttribute? target = asm
                .GetCustomAttributes(typeof(TargetFrameworkAttribute), false)
                .Cast<TargetFrameworkAttribute>()
                .FirstOrDefault();

            Assert.IsNotNull(target, "产品程序集应带 TargetFrameworkAttribute");
            StringAssert.Contains(target!.FrameworkName, "v4.8",
                "目标框架必须是 net48 —— 换框架前必须先与产品负责人确认");
        }
    }
}
