using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using System;
using uniffi.photosink_bridge;

namespace Photosink;

/// <summary>The Dates picker's rows: one per flagged asset and the date to give it.</summary>
internal sealed partial class DatesPane : Grid
{
    private readonly Core core;
    private readonly ListView list = new() { SelectionMode = ListViewSelectionMode.None };
    private readonly TextBlock empty = new() { HorizontalAlignment = HorizontalAlignment.Center, Margin = new Thickness(0, 24, 0, 0) };
    private readonly LazyList rows;
    private DatesView? view;

    internal DatesPane(Core core)
    {
        this.core = core;
        empty.Style = Ui.Style("SecondaryText");
        rows = new LazyList((start, count) => core.DatesRows((uint)start, (uint)count));
        list.ItemTemplate = (DataTemplate)Application.Current.Resources["DateRowTemplate"];
        list.ItemsSource = rows;
        list.ContainerContentChanging += (_, e) =>
        {
            if (e.ItemContainer.ContentTemplateRoot is DateRowView v && view is not null)
            {
                v.Show(rows[e.ItemIndex] as DateRow, view, core);
            }
            e.Handled = true;
        };
        Children.Add(list);
        Children.Add(empty);
    }

    private void RenderRealized(DatesView view)
    {
        if (list.ItemsPanelRoot is not ItemsStackPanel panel || panel.FirstCacheIndex < 0)
        {
            return;
        }
        for (var i = panel.FirstCacheIndex; i <= panel.LastCacheIndex && i < rows.Count; i++)
        {
            if (list.ContainerFromIndex(i) is ListViewItem item && item.ContentTemplateRoot is DateRowView v)
            {
                v.Show(rows[i] as DateRow, view, core);
            }
        }
    }

    /// <summary>Re-reads the rows; returns the first row's date options, which "Choose all" offers.</summary>
    internal DateOption[] Refresh(DatesView next)
    {
        view = next;
        var length = (int)core.DatesLen();
        rows.Refresh(length, false);
        empty.Text = next.EmptyText;
        empty.Visibility = length == 0 ? Visibility.Visible : Visibility.Collapsed;
        RenderRealized(next);
        return length > 0 && rows[0] is DateRow first ? first.Options : Array.Empty<DateOption>();
    }
}
