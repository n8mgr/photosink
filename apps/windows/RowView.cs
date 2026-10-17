using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using System;
using uniffi.photosink_bridge;

namespace Photosink;

/// <summary>One status row, rendered from a Core <c>Row</c>.</summary>
public sealed partial class RowView : Grid
{
    private readonly TextBlock position = new()
    {
        HorizontalAlignment = HorizontalAlignment.Right,
        Margin = new Thickness(0, 0, 10, 0),
    };
    private readonly StackPanel body = new() { Spacing = 1 };

    public RowView()
    {
        Padding = new Thickness(0, 3, 0, 3);
        ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(80) });
        ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        position.Style = Ui.Style("CaptionText");
        position.FontFamily = Ui.Mono;
        SetColumn(body, 1);
        Children.Add(position);
        Children.Add(body);
    }

    private void Indent(bool asset)
    {
        position.Visibility = asset ? Visibility.Visible : Visibility.Collapsed;
        SetColumn(body, asset ? 1 : 0);
        SetColumnSpan(body, asset ? 1 : 2);
    }

    private void Header(Row row, Action<string> open)
    {
        Indent(false);
        body.Children.Add(Ui.Text(row.Title, "StrongText"));
        var facts = new WrapPanel { HorizontalSpacing = 14 };
        foreach (var f in row.Facts)
        {
            var t = new TextBlock { Style = Ui.Style("SecondaryText") };
            if (f.Value.Length > 0)
            {
                t.Inlines.Add(new Run { Text = f.Value + " ", FontWeight = FontWeights.SemiBold });
            }
            t.Inlines.Add(new Run { Text = f.Label });
            if (f.Tooltip.Length > 0)
            {
                ToolTipService.SetToolTip(t, f.Tooltip);
            }
            facts.Children.Add(t);
        }
        if (row.LogPath.Length > 0)
        {
            var log = new HyperlinkButton { Content = "Log file", Padding = new Thickness(0) };
            var path = row.LogPath;
            log.Click += (_, _) => open(path);
            ToolTipService.SetToolTip(log, path);
            facts.Children.Add(log);
        }
        if (facts.Children.Count > 0)
        {
            body.Children.Add(facts);
        }
    }

    private void Asset(Row row)
    {
        Indent(true);
        position.Text = row.Position + row.Of;
        body.Children.Add(Ui.Text(row.Title, row.Failed ? "CriticalText" : "PlainText"));
        foreach (var d in row.Details)
        {
            var (prefix, style) = d.Kind switch
            {
                DetailKind.New => ("→ ", "PlainText"),
                DetailKind.Duplicate => ("= ", "SecondaryText"),
                DetailKind.Metadata => ("xmp ", "SecondaryText"),
                DetailKind.Error => ("", "CriticalText"),
                _ => ("", "SecondaryText"),
            };
            body.Children.Add(Ui.Text(prefix + d.Text, style));
        }
    }

    private void Clean(Row row)
    {
        Indent(true);
        position.Text = row.Tag;
        body.Children.Add(Ui.Text(row.Title, row.Failed ? "CriticalText" : "PlainText"));
        foreach (var d in row.Details)
        {
            body.Children.Add(Ui.Text(d.Text, d.Kind switch
            {
                DetailKind.New => "PlainText",
                DetailKind.Error => "CriticalText",
                _ => "SecondaryText",
            }));
        }
    }

    private void SignIn(Row row)
    {
        Indent(false);
        body.Children.Add(Ui.Text(row.Tag, "SecondaryText"));
        if (row.Title.Length > 0)
        {
            var code = Ui.Text(row.Title, "PlainText");
            code.FontFamily = Ui.Mono;
            code.FontSize = 28;
            code.IsTextSelectionEnabled = true;
            body.Children.Add(code);
        }
        if (Uri.TryCreate(row.Url, UriKind.Absolute, out var uri))
        {
            body.Children.Add(new HyperlinkButton { Content = row.Url, NavigateUri = uri, Padding = new Thickness(0) });
        }
    }

    internal void Show(Row? row, Action<string> open)
    {
        body.Children.Clear();
        position.Text = "";
        if (row is null)
        {
            return;
        }
        switch (row.Kind)
        {
            case RowKind.Run:
                Header(row, open);
                break;
            case RowKind.Asset:
                Asset(row);
                break;
            case RowKind.Clean:
                Clean(row);
                break;
            case RowKind.SignIn:
                SignIn(row);
                break;
            default:
                Indent(false);
                var error = Ui.Text(row.Title, "CriticalText");
                error.IsTextSelectionEnabled = true;
                body.Children.Add(error);
                break;
        }
    }
}
