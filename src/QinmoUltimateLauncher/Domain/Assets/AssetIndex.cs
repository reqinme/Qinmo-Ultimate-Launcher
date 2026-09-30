using System;
using System.Collections.Generic;

namespace Qul.Domain.Assets;

public sealed class AssetObject
{
    /// <summary>资源在游戏里的逻辑名，例如 icons/icon_16x16.png。</summary>
    public string Name { get; set; } = string.Empty;

    /// <summary>内容哈希。同时也是磁盘上的文件名与目录名来源。</summary>
    public string Hash { get; set; } = string.Empty;

    public long Size { get; set; }
}

/// <summary>
/// 资源索引。
/// 索引文件本身不含 id，id 来自版本元数据的 assetIndex.id。
/// </summary>
public sealed class AssetIndex
{
    public string Id { get; set; } = string.Empty;

    public IReadOnlyDictionary<string, AssetObject> Objects { get; set; } =
        new Dictionary<string, AssetObject>(0, StringComparer.Ordinal);

    public int Count => Objects.Count;

    /// <summary>索引内各对象体积之和。用于与版本元数据声明的 totalSize 交叉核对。</summary>
    public long TotalSize
    {
        get
        {
            long total = 0;
            foreach (AssetObject obj in Objects.Values)
            {
                total += obj.Size;
            }

            return total;
        }
    }
}
