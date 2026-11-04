using CommunityToolkit.WinUI.Controls;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using System;
using System.Linq;
using uniffi.photosink_bridge;
using Launcher = Windows.System.Launcher;
using VirtualKey = Windows.System.VirtualKey;

namespace Photosink;

/// <summary>
/// One tab: its header with the run button, its inputs as settings cards, and
/// its own status rows under a results command bar (or, on Dates, the date
/// picker). Each tab keeps one instance so its scroll position and filters
/// survive switching.
/// </summary>
internal sealed partial class TabPage : Grid
{
    private readonly Core core;
    private readonly Tab tab;
    private readonly WrapPanel subtitle = new() { HorizontalSpacing = 16, VerticalAlignment = VerticalAlignment.Center };
    private readonly InfoBar statusBar = new() { Severity = InfoBarSeverity.Error, IsClosable = false };
    private readonly InfoBar errorBar = new() { Severity = InfoBarSeverity.Error };
    private readonly CommandBar commands = new()
    {
        DefaultLabelPosition = CommandBarDefaultLabelPosition.Right,
        HorizontalAlignment = HorizontalAlignment.Stretch,
    };
    private readonly Grid runWrap = new() { Background = new Microsoft.UI.Xaml.Media.SolidColorBrush(Microsoft.UI.Colors.Transparent) };
    private readonly Button run = new() { MinWidth = 96 };
    private readonly Button apply = new() { MinWidth = 96, Visibility = Visibility.Collapsed };
    private readonly ToggleSwitch dry = new();
    private readonly AppBarButton filter = new() { Label = "Filter", Icon = new FontIcon { Glyph = "" } };
    private readonly MenuFlyout filterMenu = new();
    private readonly AppBarButton chooseAll = new() { Label = "Choose all", Icon = new FontIcon { Glyph = "" }, Visibility = Visibility.Collapsed };
    private readonly MenuFlyout chooseMenu = new();
    private readonly AppBarToggleButton follow = new() { Label = "Follow", Icon = new FontIcon { Glyph = "" }, IsChecked = true };
    private readonly AppBarButton clear = new() { Label = "Clear", Icon = new FontIcon { Glyph = "" } };
    private readonly StackPanel form = new() { Spacing = 4 };
    private readonly ContentControl formHost = new()
    {
        HorizontalContentAlignment = HorizontalAlignment.Stretch,
        IsTabStop = false,
        Margin = new Thickness(0, 8, 0, 8),
    };
    private readonly ProgressBar progress = new() { Maximum = 1, Visibility = Visibility.Collapsed };
    private readonly ListView list = new() { SelectionMode = ListViewSelectionMode.None };
    private readonly LazyList rows;
    private readonly DatesPane? dates;
    private Status? status;
    private Tab? running;
    private bool ownsStatus = true;
    private DatesView? datesView;
    private bool resetNext;

    internal TabPage(Core core, Tab tab)
    {
        this.core = core;
        this.tab = tab;
        Padding = new Thickness(24, 0, 24, 0);
        for (var i = 0; i < 5; i++)
        {
            RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        }
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });

        var heading = new StackPanel { Spacing = 2 };
        heading.Children.Add(new TextBlock { Text = Copy.TabName(tab), Style = Ui.Style("TitleTextBlockStyle") });
        heading.Children.Add(Ui.Text(Copy.Hint(tab), "SecondaryText"));
        var buttons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, VerticalAlignment = VerticalAlignment.Center };
        buttons.Children.Add(runWrap);
        buttons.Children.Add(apply);
        SetColumn(buttons, 1);
        var header = new Grid { Padding = new Thickness(0, 12, 0, 4), ColumnSpacing = 24 };
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        header.Children.Add(heading);
        header.Children.Add(buttons);

        var bars = new StackPanel { Spacing = 4 };
        bars.Children.Add(statusBar);
        bars.Children.Add(errorBar);
        SetRow(bars, 1);

        var args = core.RunArgs(tab);
        BuildForm(args);
        formHost.Content = form;
        formHost.Visibility = form.Children.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        SetRow(formHost, 2);
        SetRow(progress, 3);

        BuildCommands();
        SetRow(commands, 4);

        rows = new LazyList((start, count) => core.GridRows(tab, (uint)start, (uint)count));
        list.ItemTemplate = (DataTemplate)Application.Current.Resources["StatusRowTemplate"];
        list.ItemsSource = rows;
        list.ContainerContentChanging += (_, e) =>
        {
            if (e.ItemContainer.ContentTemplateRoot is RowView view)
            {
                view.Show(rows[e.ItemIndex] as Row, Open);
            }
            e.Handled = true;
        };
        SetRow(list, 5);

        Children.Add(header);
        Children.Add(bars);
        Children.Add(formHost);
        Children.Add(progress);
        Children.Add(commands);
        Children.Add(list);

        if (tab == Tab.Dates)
        {
            dates = new DatesPane(core) { Visibility = Visibility.Collapsed };
            SetRow(dates, 5);
            Children.Add(dates);
        }

        RefreshGrid();
        RefreshDates();
    }

    private void BuildCommands()
    {
        run.Style = Ui.Style("AccentButtonStyle");
        runWrap.Children.Add(run);
        run.Click += (_, _) => Run();
        apply.Style = Ui.Style("AccentButtonStyle");
        apply.Click += (_, _) =>
        {
            try
            {
                core.ApplyDates();
            }
            catch (CoreException e)
            {
                ShowError(Copy.ErrorText(e, tab));
            }
        };
        commands.Content = subtitle;

        filterMenu.Opening += (_, _) => BuildFilterMenu();
        filter.Flyout = filterMenu;

        chooseMenu.Opening += (_, _) => BuildChooseMenu();
        chooseAll.Flyout = chooseMenu;

        follow.Click += (_, _) =>
        {
            if (follow.IsChecked == true)
            {
                ScrollToEnd();
            }
        };
        clear.Click += (_, _) =>
        {
            resetNext = true;
            core.Clear(tab);
        };

        commands.PrimaryCommands.Add(filter);
        commands.PrimaryCommands.Add(chooseAll);
        commands.PrimaryCommands.Add(follow);
        commands.PrimaryCommands.Add(clear);
    }

    private void BuildForm(RunArgs args)
    {
        switch (tab)
        {
            case Tab.ApplePhotos:
                AddArg(args, "Library", "Photos library", "\uE8B7", FolderButton, false);
                AddDryRun(args);
                break;
            case Tab.OneDrive:
                AddArg(args, "Folder", "Folder under the drive root; empty for the whole drive", "\uE8B7", null, false);
                AddDryRun(args);
                break;
            case Tab.Dropbox:
                AddArg(args, "Folder", "Folder in your Dropbox; empty for everything", "\uE8B7", null, false);
                AddDryRun(args);
                break;
            case Tab.GoogleTakeout:
                AddArg(args, "Takeout", "Folder of archives, a single archive, or an extracted tree", "\uE8B7",
                    box => FolderOrArchiveButton(box, new[] { "zip", "tgz", "gz" }), false);
                AddDryRun(args);
                break;
            case Tab.Consolidate:
                AddDryRun(args);
                break;
            case Tab.Clean:
                AddOption(args, "Also delete files the state doesn't reference");
                AddDryRun(args);
                break;
            case Tab.Dates:
                AddArg(args, "Find", "Leave empty to list only assets whose date disagrees with the file", "\uE721", null, true);
                break;
        }
        if (tab is Tab.ApplePhotos or Tab.OneDrive or Tab.Dropbox or Tab.GoogleTakeout)
        {
            var automatic = new ToggleSwitch { IsOn = args.Automatic };
            automatic.Toggled += (_, _) => EditArgs(a => a with { Automatic = automatic.IsOn });
            form.Children.Add(new SettingsCard
            {
                Header = "Export automatically",
                HeaderIcon = new FontIcon { Glyph = "\uE823" },
                Content = automatic,
            });
        }
    }

    private void AddArg(RunArgs args, string label, string description, string glyph, Func<TextBox, FrameworkElement>? browse, bool runOnEnter)
    {
        var box = new TextBox
        {
            Text = args.Arg,
            IsSpellCheckEnabled = false,
        };
        AutomationProperties.SetName(box, label);
        box.TextChanged += (_, _) => EditArgs(a => a with { Arg = box.Text });
        if (runOnEnter)
        {
            box.KeyDown += (_, e) =>
            {
                if (e.Key == VirtualKey.Enter)
                {
                    e.Handled = true;
                    Run();
                }
            };
        }
        var row = new Grid { ColumnSpacing = 8 };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        row.Children.Add(box);
        if (browse?.Invoke(box) is { } button)
        {
            SetColumn(button, 1);
            row.Children.Add(button);
        }
        form.Children.Add(new SettingsCard
        {
            Header = label,
            Description = description,
            HeaderIcon = new FontIcon { Glyph = glyph },
            ContentAlignment = ContentAlignment.Vertical,
            HorizontalContentAlignment = HorizontalAlignment.Stretch,
            Content = row,
        });
    }

    private void AddOption(RunArgs args, string label)
    {
        var toggle = new ToggleSwitch { IsOn = args.Option };
        toggle.Toggled += (_, _) => EditArgs(a => a with { Option = toggle.IsOn });
        form.Children.Add(new SettingsCard
        {
            Header = label,
            HeaderIcon = new FontIcon { Glyph = "\uE74D" },
            Content = toggle,
        });
    }

    private void AddDryRun(RunArgs args)
    {
        dry.IsOn = args.Dry;
        dry.Toggled += (_, _) => EditArgs(a => a with { Dry = dry.IsOn });
        form.Children.Add(new SettingsCard
        {
            Header = "Dry run",
            HeaderIcon = new FontIcon { Glyph = "\uE890" },
            Content = dry,
        });
    }

    private FrameworkElement FolderButton(TextBox box)
    {
        var button = new Button { Content = "Browse…" };
        button.Click += async (_, _) =>
        {
            if (await Pickers.Folder(this) is { } path)
            {
                box.Text = path;
            }
        };
        return button;
    }

    private FrameworkElement FolderOrArchiveButton(TextBox box, string[] extensions)
    {
        var folder = new MenuFlyoutItem { Text = "Folder…" };
        folder.Click += async (_, _) =>
        {
            if (await Pickers.Folder(this) is { } path)
            {
                box.Text = path;
            }
        };
        var archive = new MenuFlyoutItem { Text = "Archive…" };
        archive.Click += async (_, _) =>
        {
            if (await Pickers.File(this, extensions) is { } path)
            {
                box.Text = path;
            }
        };
        var menu = new MenuFlyout();
        menu.Items.Add(folder);
        menu.Items.Add(archive);
        return new DropDownButton { Content = "Browse", Flyout = menu };
    }

    private void BuildFilterMenu()
    {
        filterMenu.Items.Clear();
        var chips = core.Chips(tab);
        var only = new MenuFlyoutSubItem { Text = "Show only" };
        foreach (var chip in chips)
        {
            var state = chip.State;
            var label = Copy.ChipLabel(state);
            var item = new ToggleMenuFlyoutItem { Text = $"{label}  {Copy.Count(chip.Count)}", IsChecked = !chip.Hidden };
            item.Click += (_, _) => ToggleChip(state, false);
            filterMenu.Items.Add(item);
            var just = new MenuFlyoutItem { Text = label };
            just.Click += (_, _) => ToggleChip(state, true);
            only.Items.Add(just);
        }
        filterMenu.Items.Add(new MenuFlyoutSeparator());
        filterMenu.Items.Add(only);
        var all = new MenuFlyoutItem { Text = "Show all", IsEnabled = chips.Any(c => c.Hidden) };
        all.Click += (_, _) =>
        {
            resetNext = true;
            core.ShowAll(tab);
        };
        filterMenu.Items.Add(all);
    }

    private void BuildChooseMenu()
    {
        chooseMenu.Items.Clear();
        foreach (var key in Copy.DateKeys)
        {
            var item = new MenuFlyoutItem { Text = Copy.KeyLabel(key) };
            item.Click += (_, _) => core.DatesPickAll(key);
            chooseMenu.Items.Add(item);
        }
        chooseMenu.Items.Add(new MenuFlyoutSeparator());
        var none = new MenuFlyoutItem { Text = "None" };
        none.Click += (_, _) => core.DatesClear();
        chooseMenu.Items.Add(none);
    }

    private void ToggleChip(RowState state, bool only)
    {
        resetNext = true;
        core.ToggleChip(tab, state, only);
    }

    private void EditArgs(Func<RunArgs, RunArgs> change)
    {
        var args = core.RunArgs(tab);
        var next = change(args);
        if (next != args)
        {
            try
            {
                core.SetRunArgs(tab, next);
                RefreshTask();
            }
            catch (CoreException e)
            {
                ShowError(Copy.ErrorText(e, tab));
            }
        }
    }

    private async void Run()
    {
        if (core.Blocker(tab) is not null)
        {
            return;
        }
        if (core.Confirmation(tab) is { } confirmation)
        {
            var (title, message, ok) = Copy.ConfirmationText(confirmation);
            var dialog = new ContentDialog
            {
                XamlRoot = XamlRoot,
                Style = Ui.Style("DefaultContentDialogStyle"),
                Title = title,
                Content = new TextBlock { Text = message, TextWrapping = TextWrapping.Wrap },
                PrimaryButtonText = ok,
                CloseButtonText = "Cancel",
                DefaultButton = ContentDialogButton.Close,
            };
            if (await dialog.ShowAsync() != ContentDialogResult.Primary)
            {
                return;
            }
        }
        try
        {
            core.Start(tab);
            errorBar.IsOpen = false;
        }
        catch (CoreException e)
        {
            ShowError(Copy.ErrorText(e, tab));
        }
    }

    private async void Open(string path)
    {
        try
        {
            var file = await Windows.Storage.StorageFile.GetFileFromPathAsync(path);
            if (!await Launcher.LaunchFileAsync(file))
            {
                ShowError($"{path}: no app opens this file");
            }
        }
        catch (Exception e)
        {
            ShowError($"{path}: {e.Message}");
        }
    }

    private void ShowError(string message)
    {
        errorBar.Message = message;
        errorBar.IsOpen = true;
    }

    private void ScrollToEnd()
    {
        if (rows.Count > 0 && rows[rows.Count - 1] is { } last)
        {
            list.ScrollIntoView(last);
        }
    }

    private void RenderRealized()
    {
        if (list.ItemsPanelRoot is not ItemsStackPanel panel || panel.FirstCacheIndex < 0)
        {
            return;
        }
        for (var i = panel.FirstCacheIndex; i <= panel.LastCacheIndex && i < rows.Count; i++)
        {
            if (list.ContainerFromIndex(i) is ListViewItem item && item.ContentTemplateRoot is RowView view)
            {
                view.Show(rows[i] as Row, Open);
            }
        }
    }

    private void RefreshSubtitle()
    {
        subtitle.Children.Clear();
        var picking = datesView?.Shown == true;
        var text = picking ? Copy.DatesSummary(datesView!) : ownsStatus && status is { } s && !Copy.IsError(s) ? Copy.StatusText(s) : "";
        if (text.Length > 0)
        {
            subtitle.Children.Add(Ui.Text(text, "SecondaryText"));
        }
        if (picking)
        {
            return;
        }
        foreach (var stat in core.Stats(tab))
        {
            var pair = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4 };
            pair.Children.Add(Ui.Text(Copy.StatLabel(stat.Kind), "SecondaryText"));
            pair.Children.Add(Ui.Text(Copy.Count(stat.Count), "StrongText"));
            subtitle.Children.Add(pair);
        }
    }

    private void RefreshClear()
    {
        clear.IsEnabled = running != tab && core.GridLen(tab) > 0;
    }

    internal void RefreshTask()
    {
        run.Content = Copy.RunLabel(tab, core.RunArgs(tab));
        var blocker = core.Blocker(tab);
        run.IsEnabled = blocker is null;
        ToolTipService.SetToolTip(runWrap, blocker is null ? null : Copy.BlockerText(tab, blocker));
    }

    /// <summary>Shows the window's status; <paramref name="owns"/> is false when it belongs to another tab's run.</summary>
    internal void RefreshStatus(Status next, Tab? runningTab, bool owns)
    {
        status = next;
        running = runningTab;
        ownsStatus = owns;
        statusBar.Title = Copy.StatusText(next);
        statusBar.IsOpen = owns && Copy.IsError(next);
        formHost.IsEnabled = runningTab is null;
        RefreshTask();
        RefreshClear();
        RefreshSubtitle();
        RefreshDates();
    }

    internal void RefreshGrid()
    {
        var chips = core.Chips(tab);
        var hidden = core.HiddenRows(tab);
        filter.IsEnabled = chips.Length > 0;
        filter.Label = chips.Any(c => c.Hidden) ? $"Filter · {Copy.Count(hidden)} hidden" : "Filter";

        var added = rows.Refresh((int)core.GridLen(tab), resetNext);
        resetNext = false;
        RenderRealized();
        if (added && follow.IsChecked == true)
        {
            ScrollToEnd();
        }
        RefreshClear();
        RefreshSubtitle();
    }

    internal void RefreshDates()
    {
        if (dates is null)
        {
            return;
        }
        var view = core.DatesView();
        datesView = view;
        var picking = view.Shown;
        dates.Refresh(view);

        dates.Visibility = picking ? Visibility.Visible : Visibility.Collapsed;
        list.Visibility = picking ? Visibility.Collapsed : Visibility.Visible;
        var rowCommands = picking ? Visibility.Collapsed : Visibility.Visible;
        filter.Visibility = follow.Visibility = clear.Visibility = rowCommands;
        chooseAll.Visibility = apply.Visibility = picking ? Visibility.Visible : Visibility.Collapsed;
        chooseAll.IsEnabled = view.CanPick && view.Rows > 0;
        apply.Content = Copy.DatesApply(view);
        apply.IsEnabled = view.CanApply;
        run.Style = Ui.Style(picking ? "DefaultButtonStyle" : "AccentButtonStyle");
        progress.Visibility = view.Scanning ? Visibility.Visible : Visibility.Collapsed;
        progress.Value = view.Progress;
        RefreshSubtitle();
    }
}
