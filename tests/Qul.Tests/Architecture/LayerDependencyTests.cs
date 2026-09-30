using System;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using Microsoft.VisualStudio.TestTools.UnitTesting;

namespace Qul.Tests.Architecture;

/// <summary>
/// 分层守卫。单程序集方案用不了编译期强制，因此依赖方向由本测试守住。
/// 见 docs/P0-地基规范.md §2.3。
/// </summary>
[TestClass]
public sealed class LayerDependencyTests
{
    private const BindingFlags Declared =
        BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static | BindingFlags.DeclaredOnly;

    /// <summary>领域层不得出现的命名空间。领域层是纯模型与规则，一旦沾上这些就不再可测。</summary>
    private static readonly string[] ForbiddenInDomain =
    {
        "System.Windows",
        "System.IO",
        "System.Net",
        "System.Web",
        "System.Diagnostics",
        "System.Drawing",
        "System.Runtime.InteropServices",
        "Microsoft.Win32",
    };

    [TestMethod]
    public void DomainLayer_HasTypes_SoTheGuardIsNotVacuous()
    {
        Type[] domainTypes = GetTypesInNamespace("Qul.Domain");

        Assert.IsTrue(domainTypes.Length >= 5, "领域层类型数量异常偏少，守卫可能已经失效");
    }

    [TestMethod]
    public void DomainLayer_DoesNotReferenceForbiddenNamespaces()
    {
        List<string> violations = new List<string>();

        foreach (Type type in GetTypesInNamespace("Qul.Domain"))
        {
            foreach (Type referenced in ReferencedTypes(type))
            {
                foreach (Type flattened in Flatten(referenced))
                {
                    // 跳过 COM 导入接口（例如 Exception 自带的 _Exception）：
                    // 那是框架基类带进来的，不是我们自己的分层违规。
                    if (flattened.IsInterface && flattened.Name.StartsWith("_", StringComparison.Ordinal))
                    {
                        continue;
                    }

                    string? ns = flattened.Namespace;
                    if (ns == null)
                    {
                        continue;
                    }

                    foreach (string forbidden in ForbiddenInDomain)
                    {
                        if (ns == forbidden || ns.StartsWith(forbidden + ".", StringComparison.Ordinal))
                        {
                            violations.Add(type.FullName + " -> " + flattened.FullName);
                        }
                    }
                }
            }
        }

        violations = violations.Distinct().ToList();

        Assert.AreEqual(
            0,
            violations.Count,
            "领域层出现了被禁命名空间引用：" + Environment.NewLine + string.Join(Environment.NewLine, violations.Take(15)));
    }

    private static Type[] GetTypesInNamespace(string namespacePrefix)
    {
        Assembly asm = Assembly.Load("QinmoUltimateLauncher");

        Type[] all;
        try
        {
            all = asm.GetTypes();
        }
        catch (ReflectionTypeLoadException ex)
        {
            all = ex.Types.Where(t => t != null).Select(t => t!).ToArray();
        }

        return all
            .Where(t => t.Namespace != null && (t.Namespace == namespacePrefix || t.Namespace!.StartsWith(namespacePrefix + ".", StringComparison.Ordinal)))
            .ToArray();
    }

    private static IEnumerable<Type> ReferencedTypes(Type type)
    {
        if (type.BaseType != null)
        {
            yield return type.BaseType;
        }

        foreach (Type itf in type.GetInterfaces())
        {
            yield return itf;
        }

        foreach (FieldInfo field in type.GetFields(Declared))
        {
            yield return field.FieldType;
        }

        foreach (PropertyInfo property in type.GetProperties(Declared))
        {
            yield return property.PropertyType;
        }

        foreach (MethodInfo method in type.GetMethods(Declared))
        {
            yield return method.ReturnType;

            foreach (ParameterInfo parameter in method.GetParameters())
            {
                yield return parameter.ParameterType;
            }
        }

        foreach (ConstructorInfo ctor in type.GetConstructors(Declared))
        {
            foreach (ParameterInfo parameter in ctor.GetParameters())
            {
                yield return parameter.ParameterType;
            }
        }
    }

    /// <summary>展开泛型实参与数组元素，否则 List&lt;HttpClient&gt; 这类间接引用会被漏掉。</summary>
    private static IEnumerable<Type> Flatten(Type type)
    {
        yield return type;

        if (type.IsGenericType)
        {
            foreach (Type argument in type.GetGenericArguments())
            {
                foreach (Type nested in Flatten(argument))
                {
                    yield return nested;
                }
            }
        }

        if (type.IsArray && type.GetElementType() is Type element)
        {
            foreach (Type nested in Flatten(element))
            {
                yield return nested;
            }
        }
    }
}
