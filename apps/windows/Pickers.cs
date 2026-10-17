using Microsoft.UI;
using Microsoft.UI.Xaml;
using System;
using System.Threading.Tasks;
using Windows.Storage.Pickers;

namespace Photosink;

internal static class Pickers
{
    internal static nint Hwnd(UIElement element) =>
        Win32Interop.GetWindowFromWindowId(element.XamlRoot.ContentIslandEnvironment.AppWindowId);

    internal static async Task<string?> Folder(UIElement owner)
    {
        var picker = new FolderPicker();
        picker.FileTypeFilter.Add("*");
        WinRT.Interop.InitializeWithWindow.Initialize(picker, Hwnd(owner));
        return (await picker.PickSingleFolderAsync())?.Path;
    }

    internal static async Task<string?> File(UIElement owner, string[] extensions)
    {
        var picker = new FileOpenPicker();
        foreach (var e in extensions)
        {
            picker.FileTypeFilter.Add("." + e);
        }
        if (extensions.Length == 0)
        {
            picker.FileTypeFilter.Add("*");
        }
        WinRT.Interop.InitializeWithWindow.Initialize(picker, Hwnd(owner));
        return (await picker.PickSingleFileAsync())?.Path;
    }
}
