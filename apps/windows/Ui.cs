using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Photosink;

internal static class Ui
{
    internal static FontFamily Mono => (FontFamily)Application.Current.Resources["MonoFont"];

    internal static Style Style(string key) => (Style)Application.Current.Resources[key];

    internal static TextBlock Text(string text, string style = "PlainText") => new()
    {
        Text = text,
        Style = Style(style),
        TextWrapping = TextWrapping.Wrap,
    };
}
