using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using System;
using System.Collections.Generic;
using uniffi.photosink_bridge;

namespace Photosink;

/// <summary>One asset in the Dates picker: its current date and the dates it could take.</summary>
public sealed partial class DateRowView : Grid
{
    private readonly TextBlock folder = new() { TextTrimming = TextTrimming.CharacterEllipsis };
    private readonly TextBlock stem = new() { TextTrimming = TextTrimming.CharacterEllipsis };
    private readonly TextBlock current = new() { VerticalAlignment = VerticalAlignment.Center };
    private readonly StackPanel options = new() { Orientation = Orientation.Horizontal, Spacing = 4 };
    private readonly List<ToggleButton> buttons = new();
    private readonly CalendarDatePicker manualDate = new() { PlaceholderText = "Manual date", VerticalAlignment = VerticalAlignment.Center };
    private readonly TimePicker manualTime = new() { VerticalAlignment = VerticalAlignment.Center };
    private readonly Button clearManual = new() { Content = new FontIcon { Glyph = "", FontSize = 12 }, VerticalAlignment = VerticalAlignment.Center };
    private readonly FontIcon chosen = new() { Glyph = "", FontSize = 14, VerticalAlignment = VerticalAlignment.Center };
    private DateKey[] keys = Array.Empty<DateKey>();
    private Core? core;
    private string uuid = "";
    private long? manual;
    private bool updating;

    public DateRowView()
    {
        Padding = new Thickness(0, 3, 0, 3);
        ColumnSpacing = 12;
        ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star), MinWidth = 160 });
        ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        folder.Style = Ui.Style("CaptionText");
        stem.Style = Ui.Style("PlainText");
        var name = new StackPanel { VerticalAlignment = VerticalAlignment.Center };
        name.Children.Add(folder);
        name.Children.Add(stem);

        current.Style = Ui.Style("SecondaryText");
        current.FontFamily = Ui.Mono;
        SetColumn(current, 1);
        SetColumn(options, 2);

        AutomationProperties.SetName(manualDate, "Manual date");
        AutomationProperties.SetName(manualTime, "Manual time");
        AutomationProperties.SetName(clearManual, "Clear manual date");
        ToolTipService.SetToolTip(clearManual, "Clear manual date");
        manualDate.DateChanged += (_, _) => Commit();
        manualTime.SelectedTimeChanged += (_, _) => Commit();
        clearManual.Click += (_, _) => core?.DatesManual(uuid, null);
        var field = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, VerticalAlignment = VerticalAlignment.Center };
        field.Children.Add(manualDate);
        field.Children.Add(manualTime);
        field.Children.Add(clearManual);
        field.Children.Add(chosen);
        SetColumn(field, 3);

        Children.Add(name);
        Children.Add(current);
        Children.Add(options);
        Children.Add(field);
    }

    private void Commit()
    {
        if (core is null || updating)
        {
            return;
        }
        long? at = null;
        if (manualDate.Date is { } day)
        {
            var wall = new DateTime(day.Year, day.Month, day.Day, 0, 0, 0, DateTimeKind.Utc) + (manualTime.SelectedTime ?? TimeSpan.Zero);
            at = new DateTimeOffset(wall, TimeSpan.Zero).ToUnixTimeSeconds();
        }
        if (at == manual)
        {
            return;
        }
        manual = at;
        core.DatesManual(uuid, at);
    }

    private ToggleButton Option(int index)
    {
        var caption = new TextBlock { Style = Ui.Style("CaptionText") };
        var value = new TextBlock { FontFamily = Ui.Mono };
        var content = new StackPanel();
        content.Children.Add(caption);
        content.Children.Add(value);
        var b = new ToggleButton { Content = content, VerticalAlignment = VerticalAlignment.Stretch };
        b.Click += (_, _) => core?.DatesPick(uuid, keys[index]);
        return b;
    }

    internal void Show(DateRow? row, DatesView view, Core core)
    {
        this.core = core;
        Visibility = row is null ? Visibility.Collapsed : Visibility.Visible;
        if (row is null)
        {
            return;
        }
        uuid = row.Uuid;
        folder.Text = row.Folder + "/";
        stem.Text = row.Video ? row.Stem + " ▸" : row.Stem;
        ToolTipService.SetToolTip(stem, row.Uuid);
        current.Text = row.Current is { } at ? Copy.Date(at) : "no date";

        keys = Array.ConvertAll(row.Options, o => o.Key);
        while (buttons.Count < row.Options.Length)
        {
            var b = Option(buttons.Count);
            buttons.Add(b);
            options.Children.Add(b);
        }
        for (var i = 0; i < buttons.Count; i++)
        {
            var b = buttons[i];
            b.Visibility = i < row.Options.Length ? Visibility.Visible : Visibility.Collapsed;
            if (i >= row.Options.Length)
            {
                continue;
            }
            var o = row.Options[i];
            var content = (StackPanel)b.Content;
            ((TextBlock)content.Children[0]).Text = Copy.KeyLabel(o.Key);
            ((TextBlock)content.Children[1]).Text = o.Value is { } v ? Copy.Date(v) : "—";
            b.IsChecked = o.Chosen;
            b.IsEnabled = o.Value is not null && view.CanPick;
            b.Opacity = o.SameDay ? 0.55 : 1;
        }

        updating = true;
        manual = row.Manual;
        if (row.Manual is { } m)
        {
            var wall = DateTimeOffset.FromUnixTimeSeconds(m).UtcDateTime;
            var day = DateTime.SpecifyKind(wall.Date, DateTimeKind.Unspecified);
            manualDate.Date = new DateTimeOffset(day, TimeZoneInfo.Local.GetUtcOffset(day));
            manualTime.SelectedTime = wall.TimeOfDay;
        }
        else
        {
            manualDate.Date = null;
            manualTime.SelectedTime = null;
        }
        updating = false;
        manualDate.IsEnabled = manualTime.IsEnabled = view.CanPick;
        clearManual.Visibility = row.Manual is null ? Visibility.Collapsed : Visibility.Visible;
        clearManual.IsEnabled = view.CanPick;
        chosen.Visibility = row.ManualChosen ? Visibility.Visible : Visibility.Collapsed;
    }
}
