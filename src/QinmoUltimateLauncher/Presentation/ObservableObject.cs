using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.CompilerServices;

namespace Qul.Presentation;

/// <summary>最小可用的 MVVM 基类。不引入任何第三方框架。</summary>
public abstract class ObservableObject : INotifyPropertyChanged
{
    public event PropertyChangedEventHandler? PropertyChanged;

    protected void Raise([CallerMemberName] string? propertyName = null)
    {
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
    }

    protected void RaiseAll(params string[] propertyNames)
    {
        for (int i = 0; i < propertyNames.Length; i++)
        {
            Raise(propertyNames[i]);
        }
    }

    protected bool Set<T>(ref T field, T value, [CallerMemberName] string? propertyName = null)
    {
        if (EqualityComparer<T>.Default.Equals(field, value))
        {
            return false;
        }

        field = value;
        Raise(propertyName);
        return true;
    }
}
