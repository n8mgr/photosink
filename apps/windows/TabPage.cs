using CommunityToolkit.WinUI.Controls;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using System;
using System.Linq;
using Windows.System;
using uniffi.photosink_bridge;

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
    private readonly TabInfo info;
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
    private DateOption[] chooseOptions = Array.Empty<DateOption>();
    private Status? status;
    private bool ownsStatus = true;
    private DatesView? datesView;
    private bool resetNext;

    internal TabPage(Core core, TabInfo info)
    {
        this.core = core;
        this.info = info;
        Padding = new Thickness(24, 0, 24, 0);
        for (var i = 0; i < 5; i++)
        {
            RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        }
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });

        var heading = new StackPanel { Spacing = 2 };
        heading.Children.Add(new TextBlock { Text = info.Name, Style = Ui.Style("TitleTextBlockStyle") });
        heading.Children.Add(Ui.Text(info.Hint, "SecondaryText"));
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

        var args = core.RunArgs(info.Tab);
        BuildForm(args);
        formHost.Content = form;
        formHost.Visibility = form.Children.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        SetRow(formHost, 2);
        SetRow(progress, 3);

        BuildCommands();
        SetRow(commands, 4);

        rows = new LazyList((start, count) => core.GridRows(info.Tab, (uint)start, (uint)count));
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

        if (info.Tab == Tab.Dates)
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
                ShowError(e.Message);
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
            core.Clear(info.Tab);
        };

        commands.PrimaryCommands.Add(filter);
        commands.PrimaryCommands.Add(chooseAll);
        commands.PrimaryCommands.Add(follow);
        commands.PrimaryCommands.Add(clear);
    }

    private void BuildForm(RunArgs args)
    {
        if (info.Arg is { } spec)
        {
            var box = new TextBox
            {
                Text = args.Arg,
                IsSpellCheckEnabled = false,
            };
            AutomationProperties.SetName(box, spec.Label);
            box.TextChanged += (_, _) => EditArgs(a => a with { Arg = box.Text });
            box.KeyDown += (_, e) =>
            {
                if (e.Key == VirtualKey.Enter && spec.RunOnEnter)
                {
                    e.Handled = true;
                    Run();
                }
            };
            var row = new Grid { ColumnSpacing = 8 };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.Children.Add(box);
            if (Browse(spec, box) is { } browse)
            {
                SetColumn(browse, 1);
                row.Children.Add(browse);
            }
            form.Children.Add(new SettingsCard
            {
                Header = spec.Label,
                Description = spec.Placeholder,
                HeaderIcon = new FontIcon { Glyph = spec.RunOnEnter ? "\uE721" : "\uE8B7" },
                ContentAlignment = ContentAlignment.Vertical,
                HorizontalContentAlignment = HorizontalAlignment.Stretch,
                Content = row,
            });
        }
        if (info.Option is { } label)
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
        if (info.DryRun)
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
    }

    private FrameworkElement? Browse(ArgSpec spec, TextBox box)
    {
        switch (spec.Picker)
        {
            case PickerKind.None:
                return null;
            case PickerKind.FolderOrArchive:
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
                    if (await Pickers.File(this, spec.Extensions) is { } path)
                    {
                        box.Text = path;
                    }
                };
                var menu = new MenuFlyout();
                menu.Items.Add(folder);
                menu.Items.Add(archive);
                return new DropDownButton { Content = "Browse", Flyout = menu };
            default:
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
    }

    private void BuildFilterMenu()
    {
        filterMenu.Items.Clear();
        var chips = core.Chips(info.Tab);
        var only = new MenuFlyoutSubItem { Text = "Show only" };
        foreach (var chip in chips)
        {
            var state = chip.State;
            var item = new ToggleMenuFlyoutItem { Text = $"{chip.Label}  {chip.Count}", IsChecked = !chip.Hidden };
            item.Click += (_, _) => ToggleChip(state, false);
            filterMenu.Items.Add(item);
            var just = new MenuFlyoutItem { Text = chip.Label };
            just.Click += (_, _) => ToggleChip(state, true);
            only.Items.Add(just);
        }
        filterMenu.Items.Add(new MenuFlyoutSeparator());
        filterMenu.Items.Add(only);
        var all = new MenuFlyoutItem { Text = "Show all", IsEnabled = chips.Any(c => c.Hidden) };
        all.Click += (_, _) =>
        {
            resetNext = true;
            core.ShowAll(info.Tab);
        };
        filterMenu.Items.Add(all);
    }

    private void BuildChooseMenu()
    {
        chooseMenu.Items.Clear();
        foreach (var option in chooseOptions)
        {
            var key = option.Key;
            var item = new MenuFlyoutItem { Text = option.Label };
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
        core.ToggleChip(info.Tab, state, only);
    }

    private void EditArgs(Func<RunArgs, RunArgs> change)
    {
        var args = core.RunArgs(info.Tab);
        var next = change(args);
        if (next != args)
        {
            core.SetRunArgs(info.Tab, next);
            RefreshTask();
        }
    }

    private async void Run()
    {
        if (core.Blocker(info.Tab) is not null)
        {
            return;
        }
        if (core.Confirmation(info.Tab) is { } confirm)
        {
            var dialog = new ContentDialog
            {
                XamlRoot = XamlRoot,
                Style = Ui.Style("DefaultContentDialogStyle"),
                Title = confirm.Title,
                Content = new TextBlock { Text = confirm.Message, TextWrapping = TextWrapping.Wrap },
                PrimaryButtonText = confirm.Ok,
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
            core.Start(info.Tab);
            errorBar.IsOpen = false;
        }
        catch (CoreException e)
        {
            ShowError(e.Message);
        }
    }

    private void Open(string path)
    {
        try
        {
            core.OpenPath(path);
        }
        catch (CoreException e)
        {
            ShowError(e.Message);
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
        var text = picking ? datesView!.Summary : ownsStatus && status is { Error: false } s ? s.Text : "";
        if (text.Length > 0)
        {
            subtitle.Children.Add(Ui.Text(text, "SecondaryText"));
        }
        if (picking)
        {
            return;
        }
        foreach (var stat in core.Stats(info.Tab))
        {
            var pair = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4 };
            pair.Children.Add(Ui.Text(stat.Label, "SecondaryText"));
            pair.Children.Add(Ui.Text(stat.Value, "StrongText"));
            subtitle.Children.Add(pair);
        }
    }

    private void RefreshClear()
    {
        clear.IsEnabled = status?.Running != info.Tab && core.GridLen(info.Tab) > 0;
    }

    internal void RefreshTask()
    {
        run.Content = core.RunLabel(info.Tab);
        var blocker = core.Blocker(info.Tab);
        run.IsEnabled = blocker is null;
        ToolTipService.SetToolTip(runWrap, blocker);
    }

    /// <summary>Shows the window's status; <paramref name="owns"/> is false when it belongs to another tab's run.</summary>
    internal void RefreshStatus(Status next, bool owns)
    {
        status = next;
        ownsStatus = owns;
        statusBar.Title = next.Text;
        statusBar.IsOpen = owns && next.Error;
        formHost.IsEnabled = next.Running is null;
        RefreshTask();
        RefreshClear();
        RefreshSubtitle();
        RefreshDates();
    }

    internal void RefreshGrid()
    {
        var chips = core.Chips(info.Tab);
        var hidden = core.HiddenRows(info.Tab);
        filter.IsEnabled = chips.Length > 0;
        filter.Label = chips.Any(c => c.Hidden) ? $"Filter · {hidden} hidden" : "Filter";

        var added = rows.Refresh((int)core.GridLen(info.Tab), resetNext);
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
        chooseOptions = dates.Refresh(view);

        dates.Visibility = picking ? Visibility.Visible : Visibility.Collapsed;
        list.Visibility = picking ? Visibility.Collapsed : Visibility.Visible;
        var rowCommands = picking ? Visibility.Collapsed : Visibility.Visible;
        filter.Visibility = follow.Visibility = clear.Visibility = rowCommands;
        chooseAll.Visibility = apply.Visibility = picking ? Visibility.Visible : Visibility.Collapsed;
        chooseAll.IsEnabled = view.CanPick && chooseOptions.Length > 0;
        apply.Content = view.ApplyLabel;
        apply.IsEnabled = view.CanApply;
        run.Style = Ui.Style(picking ? "DefaultButtonStyle" : "AccentButtonStyle");
        progress.Visibility = view.Scanning ? Visibility.Visible : Visibility.Collapsed;
        progress.Value = view.Progress;
        RefreshSubtitle();
    }
}
