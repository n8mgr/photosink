using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Threading.Tasks;
using VirtualKey = Windows.System.VirtualKey;
using uniffi.photosink_bridge;

namespace Photosink;

public sealed partial class SettingsPage : UserControl
{
    private readonly Dictionary<TemplateKind, string[]> used = new()
    {
        [TemplateKind.Folder] = Array.Empty<string>(),
        [TemplateKind.Filename] = Array.Empty<string>(),
    };
    private Core? core;
    private Action saved = () => { };
    private Settings? settings;
    private DispatcherQueueTimer? previewTimer;
    private bool loading;
    private int previewGeneration;

    public SettingsPage()
    {
        InitializeComponent();
        Loaded += (_, _) => ReadLoginSetting();
    }

    internal bool KeepRunningAfterClose => settings?.KeepRunningAfterClose == true;

    internal void ShowBackgroundError(string message) => ShowError(message);

    private void ReadLoginSetting()
    {
        var wasLoading = loading;
        loading = true;
        try
        {
            OpenAtLoginSwitch.IsOn = Startup.Enabled;
        }
        catch (Exception e)
        {
            ShowError($"Could not read login settings: {e.Message}");
        }
        finally
        {
            loading = wasLoading;
        }
    }

    private void ShowError(string message)
    {
        ErrorBar.Message = message;
        ErrorBar.IsOpen = true;
    }

    private void ShowOptions()
    {
        DateSourceBox.ItemsSource = Copy.DateSources.Select(Copy.DateSourceLabel).ToList();
        DateSourceCard.Description = Copy.DateSourceHint;
        ZoneBox.ItemsSource = Copy.Zones.Select(Copy.ZoneLabel).ToList();
        ZoneCard.Description = Copy.ZoneHint;
        OffsetBox.PlaceholderText = Copy.OffsetPlaceholder;
        MediaBox.ItemsSource = Copy.MediaChoices.Select(Copy.MediaLabel).ToList();
        FavoritesCard.Header = Copy.Favorites;
        AlbumsBox.PlaceholderText = Copy.ListPlaceholder;
        KeywordsBox.PlaceholderText = Copy.ListPlaceholder;
        EditsBox.ItemsSource = new List<string> { Copy.EditsOff, Copy.EditsOn };
        EditsCard.Description = Copy.EditsHint;
        MetadataBox.ItemsSource = Copy.MetadataChoices.Select(Copy.MetadataLabel).ToList();
        MetadataCard.Description = Copy.MetadataHint;
        FolderBox.PlaceholderText = Copy.FolderPlaceholder;
        FilenameBox.PlaceholderText = Copy.FilenamePlaceholder;
    }

    private void BuildFieldMenu(DropDownButton button, TextBox box, TemplateKind kind, Field[] fields)
    {
        var flyout = new MenuFlyout();
        flyout.Opening += (_, _) =>
        {
            flyout.Items.Clear();
            foreach (var group in Copy.FieldGroups)
            {
                var members = fields.Where(f => f.Group == group).ToList();
                if (members.Count == 0)
                {
                    continue;
                }
                var sub = new MenuFlyoutSubItem { Text = group.ToString() };
                foreach (var f in members)
                {
                    var item = new MenuFlyoutItem { Text = f.Name };
                    if (used[kind].Contains(f.Name))
                    {
                        item.Icon = new FontIcon { Glyph = "" };
                    }
                    ToolTipService.SetToolTip(item, Copy.FieldHelp(f.Name));
                    var token = f.Token;
                    item.Click += (_, _) => InsertField(box, kind, token);
                    sub.Items.Add(item);
                }
                flyout.Items.Add(sub);
            }
        };
        button.Flyout = flyout;
    }

    private void InsertField(TextBox box, TemplateKind kind, string token)
    {
        var text = box.Text;
        var start = Math.Clamp(box.SelectionStart, 0, text.Length);
        var end = Math.Clamp(start + box.SelectionLength, start, text.Length);
        var insert = token;
        if (start > 0 && "/_-{|.".IndexOf(text[start - 1]) < 0)
        {
            insert = (kind == TemplateKind.Folder ? "/" : "_") + token;
        }
        box.Text = text[..start] + insert + text[end..];
        box.Select(start + insert.Length, 0);
        box.Focus(FocusState.Programmatic);
        CommitTemplate(kind);
    }

    private Settings Draft() => settings! with { Path = FolderBox.Text, Filename = FilenameBox.Text };

    private async void UpdatePreviews()
    {
        if (core is null || settings is null)
        {
            return;
        }
        var draft = Draft();
        var generation = ++previewGeneration;
        var c = core;
        try
        {
            var preview = await Task.Run(() => c.Preview(draft));
            if (generation != previewGeneration)
            {
                return;
            }
            FolderPreview.Show(preview, TemplateKind.Folder);
            FilenamePreview.Show(preview, TemplateKind.Filename);
            used[TemplateKind.Folder] = preview.Folder.Used;
            used[TemplateKind.Filename] = preview.Filename.Used;
        }
        catch (Exception e)
        {
            ShowError(Copy.ErrorText(e));
        }
    }

    private void SchedulePreviews()
    {
        previewTimer?.Stop();
        previewTimer?.Start();
    }

    private void ShowSettings()
    {
        var s = settings!;
        loading = true;
        ExportCard.Description = s.ExportDir;
        FolderBox.Text = s.Path;
        FilenameBox.Text = s.Filename;
        DateSourceBox.SelectedIndex = Array.IndexOf(Copy.DateSources, s.DateSource);
        ZoneBox.SelectedIndex = Array.IndexOf(Copy.Zones, s.Zone);
        OffsetBox.Text = s.Offset;
        OffsetBox.Visibility = s.Zone == ZoneMode.Fixed ? Visibility.Visible : Visibility.Collapsed;
        MediaBox.SelectedIndex = Array.IndexOf(Copy.MediaChoices, s.Media);
        FavoritesSwitch.IsOn = s.Favorites;
        FromPicker.Date = ParseDate(s.From);
        ToPicker.Date = ParseDate(s.To);
        AlbumsBox.Text = s.Albums;
        KeywordsBox.Text = s.Keywords;
        EditsBox.SelectedIndex = s.Edits ? 1 : 0;
        MetadataBox.SelectedIndex = Array.IndexOf(Copy.MetadataChoices, s.Metadata);
        AutomaticSwitch.IsOn = s.Automatic;
        IntervalBox.Value = s.CheckIntervalMinutes;
        KeepRunningSwitch.IsOn = s.KeepRunningAfterClose;
        loading = false;
        ShowSummaries();
        UpdatePreviews();
    }

    private void ShowSummaries()
    {
        var s = settings!;
        LayoutExpander.Description = Copy.LayoutSummary(s);
        DatesExpander.Description = Copy.ClockSummary(s);
        FilterExpander.Description = Copy.FilterSummary(s);
        FilesExpander.Description = Copy.FilesSummary(s);
        FilterHint.Message = Copy.FilterHint(s);
    }

    private void Edit(Func<Settings, Settings> change)
    {
        if (loading || core is null || settings is null)
        {
            return;
        }
        var next = change(settings);
        if (next == settings)
        {
            return;
        }
        try
        {
            core.SaveSettings(next);
            settings = next;
            ExportCard.Description = next.ExportDir;
            ShowSummaries();
            ErrorBar.IsOpen = false;
        }
        catch (CoreException e)
        {
            ShowError(Copy.ErrorText(e));
            return;
        }
        SchedulePreviews();
        saved();
    }

    private void CommitTemplate(TemplateKind kind)
    {
        if (core is null || settings is null)
        {
            return;
        }
        var preview = core.Preview(Draft());
        (kind == TemplateKind.Folder ? FolderPreview : FilenamePreview).Show(preview, kind);
        var check = kind == TemplateKind.Folder ? preview.Folder : preview.Filename;
        if (check.Unknown.Length > 0)
        {
            return;
        }
        var text = kind == TemplateKind.Folder ? FolderBox.Text : FilenameBox.Text;
        Edit(s => kind == TemplateKind.Folder ? s with { Path = text } : s with { Filename = text });
    }

    private static DateTimeOffset? ParseDate(string text) =>
        DateTime.TryParseExact(text.Trim(), "yyyy-MM-dd", CultureInfo.InvariantCulture, DateTimeStyles.None, out var d)
            ? new DateTimeOffset(d)
            : null;

    private static string FormatDate(DateTimeOffset? date) =>
        date?.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture) ?? "";

    private static void OnCommit(TextBox box, Action commit)
    {
        box.LostFocus += (_, _) => commit();
        box.KeyDown += (_, e) =>
        {
            if (e.Key == VirtualKey.Enter)
            {
                commit();
                e.Handled = true;
            }
        };
    }

    private static void OnPick(ComboBox box, Action<int> pick) =>
        box.SelectionChanged += (_, _) =>
        {
            if (box.SelectedIndex >= 0)
            {
                pick(box.SelectedIndex);
            }
        };

    private void Wire()
    {
        AutomaticSwitch.Toggled += (_, _) => Edit(s => s with { Automatic = AutomaticSwitch.IsOn });
        KeepRunningSwitch.Toggled += (_, _) => Edit(s => s with { KeepRunningAfterClose = KeepRunningSwitch.IsOn });
        IntervalBox.ValueChanged += (_, _) =>
        {
            if (double.IsFinite(IntervalBox.Value) && IntervalBox.Value >= 1 && IntervalBox.Value <= 1440)
            {
                var minutes = (uint)Math.Round(IntervalBox.Value, MidpointRounding.AwayFromZero);
                IntervalBox.Value = minutes;
                Edit(s => s with { CheckIntervalMinutes = minutes });
            }
        };
        OpenAtLoginSwitch.Toggled += (_, _) =>
        {
            if (loading)
            {
                return;
            }
            try
            {
                Startup.Enabled = OpenAtLoginSwitch.IsOn;
            }
            catch (Exception e)
            {
                ShowError($"Could not change login settings: {e.Message}");
                ReadLoginSetting();
            }
        };
        ExportDirBrowse.Click += async (_, _) =>
        {
            if (await Pickers.Folder(this) is { } path)
            {
                Edit(s => s with { ExportDir = path });
            }
        };

        FolderBox.TextChanged += (_, _) => SchedulePreviews();
        FilenameBox.TextChanged += (_, _) => SchedulePreviews();
        OnCommit(FolderBox, () => CommitTemplate(TemplateKind.Folder));
        OnCommit(FilenameBox, () => CommitTemplate(TemplateKind.Filename));

        OnPick(DateSourceBox, i => Edit(s => s with { DateSource = Copy.DateSources[i] }));
        OnPick(ZoneBox, i =>
        {
            OffsetBox.Visibility = Copy.Zones[i] == ZoneMode.Fixed ? Visibility.Visible : Visibility.Collapsed;
            Edit(s => s with { Zone = Copy.Zones[i] });
        });
        OnCommit(OffsetBox, () => Edit(s => s with { Offset = OffsetBox.Text.Trim() }));

        OnPick(MediaBox, i => Edit(s => s with { Media = Copy.MediaChoices[i] }));
        FavoritesSwitch.Toggled += (_, _) => Edit(s => s with { Favorites = FavoritesSwitch.IsOn });
        FromPicker.DateChanged += (_, _) => Edit(s => s with { From = FormatDate(FromPicker.Date) });
        ToPicker.DateChanged += (_, _) => Edit(s => s with { To = FormatDate(ToPicker.Date) });
        OnCommit(AlbumsBox, () => Edit(s => s with { Albums = AlbumsBox.Text }));
        OnCommit(KeywordsBox, () => Edit(s => s with { Keywords = KeywordsBox.Text }));

        OnPick(EditsBox, i => Edit(s => s with { Edits = i == 1 }));
        OnPick(MetadataBox, i => Edit(s => s with { Metadata = Copy.MetadataChoices[i] }));
    }

    /// <summary>Loads settings from <paramref name="core"/>; <paramref name="onSaved"/> runs after each save attempt.</summary>
    internal void Init(Core core, Action onSaved)
    {
        this.core = core;
        saved = onSaved;
        previewTimer = DispatcherQueue.CreateTimer();
        previewTimer.Interval = TimeSpan.FromMilliseconds(120);
        previewTimer.IsRepeating = false;
        previewTimer.Tick += (_, _) => UpdatePreviews();

        ShowOptions();
        var fields = core.Fields();
        BuildFieldMenu(FolderInsert, FolderBox, TemplateKind.Folder, fields);
        BuildFieldMenu(FilenameInsert, FilenameBox, TemplateKind.Filename, fields);
        Wire();
        try
        {
            settings = core.Settings();
        }
        catch (CoreException e)
        {
            ShowError(Copy.ErrorText(e));
            Body.Visibility = Visibility.Collapsed;
            return;
        }
        ShowSettings();
    }
}
