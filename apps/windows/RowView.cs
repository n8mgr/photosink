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

    private void ShowRun(Row.Run row, Action<string> open)
    {
        Indent(false);
        if (row.Title is { } title)
        {
            body.Children.Add(Ui.Text(Copy.RunTitleText(title), "StrongText"));
        }
        var facts = new WrapPanel { HorizontalSpacing = 14 };
        foreach (var fact in row.Facts)
        {
            var (value, label, tooltip) = Copy.FactParts(fact);
            var t = new TextBlock { Style = Ui.Style("SecondaryText") };
            if (value is not null)
            {
                t.Inlines.Add(new Run { Text = value + " ", FontWeight = FontWeights.SemiBold });
            }
            t.Inlines.Add(new Run { Text = label });
            if (tooltip is not null)
            {
                ToolTipService.SetToolTip(t, tooltip);
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

    private void ShowAsset(Row.Asset row)
    {
        Indent(true);
        position.Text = row.Position is { } p ? $"{Copy.Count(p.N)}/{Copy.Count(p.Total)}" : "";
        body.Children.Add(Ui.Text(row.Name ?? "…", row.Failed ? "CriticalText" : "PlainText"));
        foreach (var detail in row.Details)
        {
            switch (detail)
            {
                case Detail.Placed d:
                    body.Children.Add(Ui.Text("→ " + d.Path, "PlainText"));
                    break;
                case Detail.Duplicate d:
                    body.Children.Add(Ui.Text("= " + d.Existing, "SecondaryText"));
                    break;
                case Detail.Metadata d:
                    body.Children.Add(Ui.Text("xmp " + d.Path, "SecondaryText"));
                    break;
                case Detail.Redated d:
                    body.Children.Add(Ui.Text(d.From is { } before ? Copy.Date(before) : "none", "SecondaryText"));
                    body.Children.Add(Ui.Text("→ " + Copy.Date(d.To), "PlainText"));
                    break;
                case Detail.Error d:
                    body.Children.Add(Ui.Text(Copy.WithSubject(d.Subject, d.Message), "CriticalText"));
                    break;
                case Detail.NothingNew:
                    body.Children.Add(Ui.Text("nothing new", "SecondaryText"));
                    break;
            }
        }
    }

    private void ShowClean(Row.Clean row)
    {
        Indent(true);
        position.Text = Copy.ReasonLabel(row.Reason);
        body.Children.Add(Ui.Text(row.Path, row.Failed ? "CriticalText" : "PlainText"));
        body.Children.Add(Ui.Text(Copy.OutcomeLabel(row.Outcome), row.Outcome == CleanOutcome.Removed ? "PlainText" : "SecondaryText"));
    }

    private void ShowSignIn(Row.SignIn row)
    {
        Indent(false);
        switch (row.Prompt)
        {
            case SignIn.DeviceCode device:
                body.Children.Add(Ui.Text("Sign in with code", "SecondaryText"));
                var code = Ui.Text(device.Code, "PlainText");
                code.FontFamily = Ui.Mono;
                code.FontSize = 28;
                code.IsTextSelectionEnabled = true;
                body.Children.Add(code);
                break;
            default:
                body.Children.Add(Ui.Text("Approve access in your browser", "SecondaryText"));
                break;
        }
        if (Uri.TryCreate(row.Url, UriKind.Absolute, out var uri))
        {
            body.Children.Add(new HyperlinkButton { Content = row.Url, NavigateUri = uri, Padding = new Thickness(0) });
        }
    }

    private void ShowError(Row.Error row)
    {
        Indent(false);
        var error = Ui.Text(Copy.WithSubject(row.Subject, row.Message), "CriticalText");
        error.IsTextSelectionEnabled = true;
        body.Children.Add(error);
    }

    internal void Show(Row? row, Action<string> open)
    {
        body.Children.Clear();
        position.Text = "";
        switch (row)
        {
            case Row.Run run:
                ShowRun(run, open);
                break;
            case Row.Asset asset:
                ShowAsset(asset);
                break;
            case Row.Clean clean:
                ShowClean(clean);
                break;
            case Row.SignIn signIn:
                ShowSignIn(signIn);
                break;
            case Row.Error error:
                ShowError(error);
                break;
        }
    }
}
