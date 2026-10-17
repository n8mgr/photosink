using System;
using System.Collections;
using System.Collections.Generic;
using System.Collections.Specialized;

namespace Photosink;

/// <summary>
/// A read-only list of <c>Count</c> rows that fetches them from Core a page at
/// a time, so a ListView only asks for the rows it realizes.
/// </summary>
public sealed partial class LazyList : IList, INotifyCollectionChanged
{
    private const int PageSize = 64;
    private const int MaxAdds = 256;

    private readonly Func<int, int, object[]> fetch;
    private readonly Dictionary<int, object[]> pages = new();
    private int count;

    internal LazyList(Func<int, int, object[]> fetch)
    {
        this.fetch = fetch;
    }

    public event NotifyCollectionChangedEventHandler? CollectionChanged;

    public int Count => count;

    public bool IsFixedSize => false;

    public bool IsReadOnly => true;

    public bool IsSynchronized => false;

    public object SyncRoot => this;

    public object? this[int index]
    {
        get
        {
            if (index < 0 || index >= count)
            {
                return null;
            }
            var p = index / PageSize;
            if (!pages.TryGetValue(p, out var page))
            {
                page = fetch(p * PageSize, PageSize);
                pages[p] = page;
            }
            var offset = index % PageSize;
            return offset < page.Length ? page[offset] : null;
        }
        set => throw new NotSupportedException();
    }

    /// <summary>
    /// Drops cached rows and moves to <paramref name="length"/> rows, raising
    /// Add for appended rows or Reset when rows went away or were reordered.
    /// Returns whether rows were added.
    /// </summary>
    internal bool Refresh(int length, bool reset)
    {
        pages.Clear();
        var old = count;
        if (reset || length < old || length - old > MaxAdds)
        {
            count = length;
            CollectionChanged?.Invoke(this, new NotifyCollectionChangedEventArgs(NotifyCollectionChangedAction.Reset));
            return length > 0;
        }
        for (var i = old; i < length; i++)
        {
            count = i + 1;
            CollectionChanged?.Invoke(this, new NotifyCollectionChangedEventArgs(NotifyCollectionChangedAction.Add, this[i], i));
        }
        return length > old;
    }

    public int Add(object? value) => throw new NotSupportedException();

    public void Clear() => throw new NotSupportedException();

    public bool Contains(object? value) => IndexOf(value) >= 0;

    public int IndexOf(object? value)
    {
        foreach (var (p, page) in pages)
        {
            var i = Array.FindIndex(page, o => ReferenceEquals(o, value));
            if (i >= 0)
            {
                return p * PageSize + i;
            }
        }
        return -1;
    }

    public void Insert(int index, object? value) => throw new NotSupportedException();

    public void Remove(object? value) => throw new NotSupportedException();

    public void RemoveAt(int index) => throw new NotSupportedException();

    public void CopyTo(Array array, int index)
    {
        for (var i = 0; i < count; i++)
        {
            array.SetValue(this[i], index + i);
        }
    }

    public IEnumerator GetEnumerator()
    {
        for (var i = 0; i < count; i++)
        {
            yield return this[i];
        }
    }
}
