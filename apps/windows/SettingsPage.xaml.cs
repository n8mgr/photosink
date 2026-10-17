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
    private Options? options;
    private Action saved = () => { };
    private Settings? settings;
    private DispatcherQueueTimer? previewTimer;
    private bool loading;
    private int previewGeneration;

    public SettingsPage()
    {
        InitializeComponent();
    }

    private void ShowError(string message)
    {
        ErrorBar.Message = message;
        ErrorBar.IsOpen = true;
    }

    private void ShowOptions(Options o)
    {
        DateSourceBox.ItemsSource = o.DateSources.Select(x => x.Label).ToList();
        DateSourceCard.Description = o.DateSourceHint;
        ZoneBox.ItemsSource = o.Zones.Select(x => x.Label).ToList();
        ZoneCard.Description = o.ZoneHint;
        OffsetBox.PlaceholderText = o.OffsetPlaceholder;
        MediaBox.ItemsSource = o.Media.Select(x => x.Label).ToList();
        FavoritesCard.Header = o.FavoritesLabel;
        AlbumsBox.PlaceholderText = o.ListPlaceholder;
        KeywordsBox.PlaceholderText = o.ListPlaceholder;
        EditsBox.ItemsSource = new List<string> { o.EditsOff, o.EditsOn };
        EditsCard.Description = o.EditsHint;
        MetadataBox.ItemsSource = o.Metadata.Select(x => x.Label).ToList();
        MetadataCard.Description = o.MetadataHint;
        FolderBox.PlaceholderText = o.FolderPlaceholder;
        FilenameBox.PlaceholderText = o.FilenamePlaceholder;
    }

    private void BuildFieldMenu(DropDownButton button, TextBox box, TemplateKind kind, FieldGroup[] fields)
    {
        var flyout = new MenuFlyout();
        flyout.Opening += (_, _) =>
        {
            flyout.Items.Clear();
            foreach (var group in fields)
            {
                var sub = new MenuFlyoutSubItem { Text = group.Name };
                foreach (var f in group.Fields)
                {
                    var item = new MenuFlyoutItem { Text = f.Name };
                    if (used[kind].Contains(f.Name))
                    {
                        item.Icon = new FontIcon { Glyph = "" };
                    }
                    ToolTipService.SetToolTip(item, f.Help);
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
        var start = box.SelectionStart;
        var end = start + box.SelectionLength;
        var insertion = core!.InsertField(box.Text, (uint)start, (uint)end, token, kind);
        box.Text = insertion.Text;
        box.Select((int)insertion.Caret, 0);
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
            var (folder, file) = await Task.Run(() =>
                (c.Preview(draft, TemplateKind.Folder), c.Preview(draft, TemplateKind.Filename)));
            if (generation != previewGeneration)
            {
                return;
            }
            FolderPreview.Show(folder, TemplateKind.Folder);
            FilenamePreview.Show(file, TemplateKind.Filename);
            used[TemplateKind.Folder] = folder.Used;
            used[TemplateKind.Filename] = file.Used;
        }
        catch (Exception e)
        {
            ShowError(e.Message);
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
        var o = options!;
        loading = true;
        ExportCard.Description = s.ExportDir;
        FolderBox.Text = s.Path;
        FilenameBox.Text = s.Filename;
        DateSourceBox.SelectedIndex = Array.FindIndex(o.DateSources, x => x.Value == s.DateSource);
        ZoneBox.SelectedIndex = Array.FindIndex(o.Zones, x => x.Value == s.Zone);
        OffsetBox.Text = s.Offset;
        OffsetBox.Visibility = s.Zone == ZoneMode.Fixed ? Visibility.Visible : Visibility.Collapsed;
        MediaBox.SelectedIndex = Array.FindIndex(o.Media, x => x.Value == s.Media);
        FavoritesSwitch.IsOn = s.Favorites;
        FromPicker.Date = ParseDate(s.From);
        ToPicker.Date = ParseDate(s.To);
        AlbumsBox.Text = s.Albums;
        KeywordsBox.Text = s.Keywords;
        EditsBox.SelectedIndex = s.Edits ? 1 : 0;
        MetadataBox.SelectedIndex = Array.FindIndex(o.Metadata, x => x.Value == s.Metadata);
        loading = false;
        ShowSummaries();
        UpdatePreviews();
    }

    private void ShowSummaries()
    {
        var m = core!.Summaries(settings!);
        LayoutExpander.Description = m.Layout;
        DatesExpander.Description = m.Dates;
        FilterExpander.Description = m.Filter;
        FilesExpander.Description = m.Files;
        FilterHint.Message = m.FilterHint;
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
        settings = next;
        ExportCard.Description = next.ExportDir;
        ShowSummaries();
        try
        {
            core.SaveSettings(next);
            ErrorBar.IsOpen = false;
        }
        catch (CoreException e)
        {
            ShowError(e.Message);
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
        var preview = core.Preview(Draft(), kind);
        (kind == TemplateKind.Folder ? FolderPreview : FilenamePreview).Show(preview, kind);
        if (preview.Problem is not null)
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

    private void Wire(Options o)
    {
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

        OnPick(DateSourceBox, i => Edit(s => s with { DateSource = o.DateSources[i].Value }));
        OnPick(ZoneBox, i =>
        {
            OffsetBox.Visibility = o.Zones[i].Value == ZoneMode.Fixed ? Visibility.Visible : Visibility.Collapsed;
            Edit(s => s with { Zone = o.Zones[i].Value });
        });
        OnCommit(OffsetBox, () => Edit(s => s with { Offset = OffsetBox.Text.Trim() }));

        OnPick(MediaBox, i => Edit(s => s with { Media = o.Media[i].Value }));
        FavoritesSwitch.Toggled += (_, _) => Edit(s => s with { Favorites = FavoritesSwitch.IsOn });
        FromPicker.DateChanged += (_, _) => Edit(s => s with { From = FormatDate(FromPicker.Date) });
        ToPicker.DateChanged += (_, _) => Edit(s => s with { To = FormatDate(ToPicker.Date) });
        OnCommit(AlbumsBox, () => Edit(s => s with { Albums = AlbumsBox.Text }));
        OnCommit(KeywordsBox, () => Edit(s => s with { Keywords = KeywordsBox.Text }));

        OnPick(EditsBox, i => Edit(s => s with { Edits = i == 1 }));
        OnPick(MetadataBox, i => Edit(s => s with { Metadata = o.Metadata[i].Value }));
    }

    /// <summary>Loads settings from <paramref name="core"/>; <paramref name="onSaved"/> runs after each save attempt.</summary>
    internal void Init(Core core, Action onSaved)
    {
        this.core = core;
        saved = onSaved;
        options = core.Options();
        previewTimer = DispatcherQueue.CreateTimer();
        previewTimer.Interval = TimeSpan.FromMilliseconds(120);
        previewTimer.IsRepeating = false;
        previewTimer.Tick += (_, _) => UpdatePreviews();

        ShowOptions(options);
        var fields = core.Fields();
        BuildFieldMenu(FolderInsert, FolderBox, TemplateKind.Folder, fields);
        BuildFieldMenu(FilenameInsert, FilenameBox, TemplateKind.Filename, fields);
        Wire(options);
        try
        {
            settings = core.Settings();
        }
        catch (CoreException e)
        {
            ShowError(e.Message);
            Body.Visibility = Visibility.Collapsed;
            return;
        }
        ShowSettings();
    }
}
