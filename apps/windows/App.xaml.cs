using Microsoft.UI.Xaml;
using System.Threading;

namespace Photosink;

public partial class App : Application
{
    private Window? window;
    private Mutex? instance;

    public App()
    {
        InitializeComponent();
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        instance = new Mutex(true, @"Local\com.n8mgr.photosink", out var first);
        if (!first)
        {
            BackgroundWindow.OpenExisting();
            Exit();
            return;
        }
        window = new MainWindow();
        window.Activate();
    }
}
