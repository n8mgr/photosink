using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using uniffi.photosink_bridge;

namespace Photosink;

/// <summary>The rendered path a template produces, or why it can't be saved.</summary>
public sealed partial class TemplatePreview : StackPanel
{
    private readonly TextBlock path = new() { TextWrapping = TextWrapping.Wrap };
    private readonly TextBlock note = Ui.Text("", "CaptionText");
    private readonly TextBlock problem = Ui.Text("", "CriticalText");

    public TemplatePreview()
    {
        Spacing = 2;
        path.Style = Ui.Style("SecondaryText");
        path.FontFamily = Ui.Mono;
        note.FontStyle = Windows.UI.Text.FontStyle.Italic;
        Children.Add(path);
        Children.Add(note);
        Children.Add(problem);
    }

    internal void Show(Preview preview, TemplateKind kind)
    {
        var check = kind == TemplateKind.Folder ? preview.Folder : preview.Filename;
        var bad = check.Unknown.Length > 0;
        problem.Text = bad ? Copy.UnknownFields(check.Unknown) : "";
        problem.Visibility = bad ? Visibility.Visible : Visibility.Collapsed;
        path.Visibility = note.Visibility = bad ? Visibility.Collapsed : Visibility.Visible;
        path.Inlines.Clear();
        path.Inlines.Add(new Run { Text = "→ " + (preview.Root ?? "Export") + "/" });
        path.Inlines.Add(Part(preview.Dir.Length == 0 ? "…" : preview.Dir, kind == TemplateKind.Folder));
        path.Inlines.Add(new Run { Text = "/" });
        path.Inlines.Add(Part(preview.File, kind == TemplateKind.Filename));
        path.Inlines.Add(new Run { Text = "." + preview.Ext });
        note.Text = Copy.PreviewNote(preview);
    }

    private static Run Part(string text, bool edited) => new()
    {
        Text = text,
        FontWeight = edited ? FontWeights.SemiBold : FontWeights.Normal,
    };
}
