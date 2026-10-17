using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using System;
using System.Collections.Generic;
using Windows.System;
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
    private readonly TextBox manual = new() { Width = 240, IsSpellCheckEnabled = false };
    private readonly TextBlock error = new() { TextWrapping = TextWrapping.Wrap, MaxWidth = 240 };
    private readonly FontIcon chosen = new() { Glyph = "\uE73E", FontSize = 14, VerticalAlignment = VerticalAlignment.Center };
    private DateKey[] keys = Array.Empty<DateKey>();
    private Core? core;
    private string uuid = "";
    private string committed = "";

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

        error.Style = Ui.Style("CriticalText");
        manual.FontFamily = Ui.Mono;
        manual.KeyDown += (_, e) =>
        {
            if (e.Key == VirtualKey.Enter)
            {
                Commit();
                e.Handled = true;
            }
        };
        manual.LostFocus += (_, _) => Commit();
        var field = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
        field.Children.Add(manual);
        field.Children.Add(chosen);
        var typed = new StackPanel { Spacing = 2, VerticalAlignment = VerticalAlignment.Center };
        typed.Children.Add(field);
        typed.Children.Add(error);
        SetColumn(typed, 3);

        Children.Add(name);
        Children.Add(current);
        Children.Add(options);
        Children.Add(typed);
    }

    private void Commit()
    {
        if (core is null || manual.Text == committed)
        {
            return;
        }
        committed = manual.Text;
        ShowError(core.DatesManual(uuid, manual.Text) ?? "");
    }

    private void ShowError(string text)
    {
        error.Text = text;
        error.Visibility = text.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
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
        current.Text = row.Current;

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
            ((TextBlock)content.Children[0]).Text = o.Label;
            ((TextBlock)content.Children[1]).Text = o.Value.Length > 0 ? o.Value : "—";
            b.IsChecked = o.Chosen;
            b.IsEnabled = o.Value.Length > 0 && view.CanPick;
            b.Opacity = o.SameDay ? 0.55 : 1;
        }

        manual.PlaceholderText = view.ManualPlaceholder;
        manual.IsEnabled = view.CanPick;
        if (manual.FocusState == FocusState.Unfocused)
        {
            manual.Text = row.Manual;
            committed = row.Manual;
        }
        chosen.Visibility = row.ManualChosen ? Visibility.Visible : Visibility.Collapsed;
        ShowError(row.ManualError);
    }
}
