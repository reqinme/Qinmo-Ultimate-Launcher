using Microsoft.VisualStudio.TestTools.UnitTesting;

// 测试并行度：方法级并行。
// P1 起版本元数据解析用例会成批出现，串行会明显拖慢"一次构建带上全部检查"这条纪律。
[assembly: Parallelize(Scope = ExecutionScope.MethodLevel)]
