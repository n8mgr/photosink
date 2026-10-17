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

    public MainWindow()
    {
        InitializeComponent();
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        AppWindow.Resize(new Windows.Graphics.SizeInt32(1200, 860));
        AppTitleBar.PaneToggleRequested += (_, _) => Nav.IsPaneOpen = !Nav.IsPaneOpen;

        core = new Core(new CoreListener(DispatcherQueue, OnChanged));
        Closed += (_, _) =>
        {
            closed = true;
            core.Dispose();
        };

        SettingsView.Init(core, RefreshTasks);
        foreach (var group in core.Groups())
        {
            Nav.MenuItems.Add(new NavigationViewItemHeader { Content = group.Name });
            foreach (var info in group.Tabs)
            {
                var item = new NavigationViewItem
                {
                    Content = info.Name,
                    Icon = new FontIcon { Glyph = Glyph(info.Tab) },
                    Tag = (int)info.Tab,
                };
                var page = new TabPage(core, info) { Visibility = Visibility.Collapsed };
                items[info.Tab] = item;
                pages[info.Tab] = page;
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
        lastRun = status.Running ?? lastRun;
        foreach (var (tab, item) in items)
        {
            item.IsEnabled = status.Running is null || status.Running == tab;
        }
        foreach (var (tab, page) in pages)
        {
            page.RefreshStatus(status, lastRun is null || lastRun == tab);
        }
    }

    private void RefreshTasks()
    {
        foreach (var page in pages.Values)
        {
            page.RefreshTask();
        }
    }
}
