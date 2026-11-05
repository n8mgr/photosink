using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using System;
using System.Collections.Generic;
using System.Linq;
using uniffi.photosink_bridge;

namespace Photosink;

public sealed partial class MainWindow : Window
{
    private readonly Core core;
    private readonly Dictionary<Tab, TabPage> pages = new();
    private readonly Dictionary<Tab, NavigationViewItem> items = new();
    private Tab? lastRun;
    private bool closed;
    private bool quitting;
    private readonly BackgroundWindow background;

    public MainWindow()
    {
        InitializeComponent();
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        AppWindow.Resize(new Windows.Graphics.SizeInt32(1200, 860));
        AppTitleBar.PaneToggleRequested += (_, _) => Nav.IsPaneOpen = !Nav.IsPaneOpen;

        core = new Core(new CoreListener(DispatcherQueue, OnChanged));
        background = new BackgroundWindow(
            WinRT.Interop.WindowNative.GetWindowHandle(this),
            () => DispatcherQueue.TryEnqueue(Open),
            () => DispatcherQueue.TryEnqueue(Quit));
        AppWindow.Closing += (_, e) =>
        {
            if (!quitting && SettingsView.KeepRunningAfterClose)
            {
                e.Cancel = true;
                if (background.SetVisible(true))
                {
                    AppWindow.Hide();
                }
                else
                {
                    SettingsView.ShowBackgroundError("Could not add the notification area icon. Photosink will stay open.");
                }
            }
        };
        Closed += (_, _) =>
        {
            closed = true;
            background.Dispose();
            core.Dispose();
        };

        SettingsView.Init(core, RefreshTasks);
        var available = core.Tabs();
        foreach (var (name, tabs) in Copy.Groups)
        {
            var present = tabs.Where(t => Array.IndexOf(available, t) >= 0).ToList();
            if (present.Count == 0)
            {
                continue;
            }
            Nav.MenuItems.Add(new NavigationViewItemHeader { Content = name });
            foreach (var tab in present)
            {
                var item = new NavigationViewItem
                {
                    Content = Copy.TabName(tab),
                    Icon = new FontIcon { Glyph = Glyph(tab) },
                    Tag = (int)tab,
                };
                var page = new TabPage(core, tab) { Visibility = Visibility.Collapsed };
                items[tab] = item;
                pages[tab] = page;
                Nav.MenuItems.Add(item);
                Pages.Children.Add(page);
            }
        }
        Nav.SelectionChanged += (_, e) => Show(e.IsSettingsSelected ? null : (e.SelectedItem as NavigationViewItem)?.Tag is int t ? (Tab)t : null);
        if (items.Count > 0)
        {
            Nav.SelectedItem = items.Values.First();
        }
        RefreshStatus();
        RefreshBackground();
    }

    private void Open()
    {
        AppWindow.Show();
        if (AppWindow.Presenter is OverlappedPresenter { State: OverlappedPresenterState.Minimized } presenter)
        {
            presenter.Restore();
        }
        Activate();
        BackgroundWindow.SetForegroundWindow(WinRT.Interop.WindowNative.GetWindowHandle(this));
    }

    private void Quit()
    {
        quitting = true;
        Close();
    }

    private void RefreshBackground()
    {
        if (!background.SetVisible(SettingsView.KeepRunningAfterClose))
        {
            SettingsView.ShowBackgroundError("Could not add the notification area icon.");
        }
    }

    private static string Glyph(Tab tab) => tab switch
    {
        Tab.ApplePhotos => "",
        Tab.OneDrive => "",
        Tab.Dropbox => "\uE896",
        Tab.GoogleTakeout => "",
        Tab.Consolidate => "",
        Tab.Clean => "",
        Tab.Dates => "",
        _ => "",
    };

    private void Show(Tab? tab)
    {
        SettingsView.Visibility = tab is null ? Visibility.Visible : Visibility.Collapsed;
        foreach (var (t, page) in pages)
        {
            page.Visibility = t == tab ? Visibility.Visible : Visibility.Collapsed;
        }
    }

    private void OnChanged(Change change)
    {
        if (closed)
        {
            return;
        }
        switch (change)
        {
            case Change.Status _:
                RefreshStatus();
                break;
            case Change.Grid grid:
                if (pages.TryGetValue(grid.Tab, out var page))
                {
                    page.RefreshGrid();
                }
                break;
            case Change.Dates _:
                if (pages.TryGetValue(Tab.Dates, out var dates))
                {
                    dates.RefreshDates();
                }
                break;
        }
    }

    private void RefreshStatus()
    {
        var status = core.Status();
        var running = core.Running();
        lastRun = running ?? lastRun;
        foreach (var (tab, page) in pages)
        {
            page.RefreshStatus(status, running, lastRun is null || lastRun == tab);
        }
    }

    private void RefreshTasks()
    {
        RefreshBackground();
        foreach (var page in pages.Values)
        {
            page.RefreshTask();
        }
    }
}
