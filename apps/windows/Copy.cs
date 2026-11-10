using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using uniffi.photosink_bridge;

namespace Photosink;

/// <summary>Every user-facing string the app builds from the bridge's typed data.</summary>
internal static class Copy
{
    internal static readonly (string Name, Tab[] Tabs)[] Groups =
    {
        ("Import", new[] { Tab.ApplePhotos, Tab.OneDrive, Tab.Dropbox, Tab.GoogleTakeout }),
        ("Maintain", new[] { Tab.Consolidate, Tab.Clean, Tab.Dates }),
    };

    internal static readonly DateSource[] DateSources = { DateSource.Source, DateSource.File };
    internal static readonly ZoneMode[] Zones = { ZoneMode.Asset, ZoneMode.Local, ZoneMode.Utc, ZoneMode.Fixed };
    internal static readonly Media[] MediaChoices = { Media.All, Media.Photos, Media.Videos };
    internal static readonly Metadata[] MetadataChoices = { Metadata.Both, Metadata.Embed, Metadata.Sidecar };
    internal static readonly FieldGroup[] FieldGroups = { FieldGroup.Date, FieldGroup.Place, FieldGroup.Tags, FieldGroup.Camera, FieldGroup.Other };
    internal static readonly DateKey[] DateKeys = { DateKey.Create, DateKey.Original, DateKey.Modify };

    internal const string DateSourceHint = "falls back to the other, then the upload time";
    internal const string ZoneHint = "the zone the folder date is read in";
    internal const string OffsetPlaceholder = "+02:00";
    internal const string Favorites = "Favourites only";
    internal const string ListPlaceholder = "Comma separated; any match";
    internal const string EditsOff = "Originals";
    internal const string EditsOn = "Originals + edits";
    internal const string EditsHint = "edits are the rendered version plus the .AAE adjustment data";
    internal const string MetadataHint = "the XMP packet written into each file, to a .xmp beside it, or both";
    internal const string FolderPlaceholder = "{take.year}/{take.month}";
    internal const string FilenamePlaceholder = "{stem}";

    private const string Separator = " · ";

    internal static string Count(ulong n) => n.ToString("N0", CultureInfo.CurrentCulture);

    internal static string Date(long seconds) =>
        DateTimeOffset.FromUnixTimeSeconds(seconds).UtcDateTime.ToString("G", CultureInfo.CurrentCulture);

    internal static string TabName(Tab tab) => tab switch
    {
        Tab.ApplePhotos => "Apple Photos",
        Tab.OneDrive => "OneDrive",
        Tab.Dropbox => "Dropbox",
        Tab.GoogleTakeout => "Google Takeout",
        Tab.Consolidate => "Consolidate",
        Tab.Clean => "Clean",
        Tab.Dates => "Dates",
        _ => tab.ToString(),
    };

    internal static string Hint(Tab tab) => tab switch
    {
        Tab.ApplePhotos => "Exports originals and metadata from the Photos library through PhotoKit.",
        Tab.OneDrive => "Signs in with a device code the first time, then exports what changed since the last run.",
        Tab.Dropbox => "Signs in through the browser the first time, then exports what changed since the last run.",
        Tab.GoogleTakeout => "Reads the archives in place; nothing needs extracting first.",
        Tab.Consolidate => "Moves everything already exported into the folders the current path template produces.",
        Tab.Clean => "Rows whose file is gone are forgotten; an asset left with no files loses its sidecar and is exported again next run. Sidecars without an asset are deleted. Dry run only lists.",
        Tab.Dates => "Matches folder, name or original filename. Pick a date per asset, then Apply stores it, rewrites metadata and moves the files.",
        _ => "",
    };

    internal static string RunLabel(Tab tab, RunArgs args) => tab switch
    {
        Tab.Consolidate => "Consolidate",
        Tab.Clean => args.Dry ? "List" : "Remove",
        Tab.Dates => args.Arg.Trim().Length == 0 ? "Scan" : "Find",
        _ => "Run",
    };

    internal static string BlockerText(Tab tab, Blocker blocker) => blocker switch
    {
        Blocker.Running r => $"{TabName(r.Tab)} is running.",
        Blocker.NoExportFolder => "Choose a folder to export to first.",
        Blocker.ArgRequired => tab switch
        {
            Tab.ApplePhotos => "Choose a library first.",
            Tab.GoogleTakeout => "Choose a takeout first.",
            _ => "Choose a folder first.",
        },
        Blocker.DropboxKeyMissing d => $"Set dropbox_app_key in {d.ConfigPath} to your Dropbox app's key.",
        Blocker.Unavailable => "Apple Photos is only available on macOS",
        _ => "",
    };

    internal static (string Title, string Message, string Ok) ConfirmationText(Confirmation confirmation) => confirmation switch
    {
        Confirmation.CleanDeletes { Unreferenced: true } => (
            "Clean the export",
            "Delete sidecars whose files are gone, forget their rows, and delete every media file the state does not reference. This cannot be undone.",
            "Delete"),
        _ => (
            "Clean the export",
            "Delete sidecars whose files are gone and forget their rows. This cannot be undone.",
            "Delete"),
    };

    internal static string StatusText(Status status) => status switch
    {
        Status.Idle => "Idle",
        Status.Running r => $"Running {TabName(r.Tab)}…",
        Status.CheckingDates => "Checking dates…",
        Status.ApplyingDates => "Applying dates…",
        Status.Done { Errors: 0 } => "Done",
        Status.Done d => $"Done with {Count(d.Errors)} errors",
        Status.Failed => "Failed",
        _ => "",
    };

    internal static bool IsError(Status status) => status is Status.Failed or Status.Done { Errors: > 0 };

    internal static string StatLabel(StatKind kind) => kind switch
    {
        StatKind.Exported => "exported",
        StatKind.Planned => "planned",
        StatKind.Duplicates => "duplicates",
        StatKind.Skipped => "skipped",
        StatKind.Filtered => "filtered",
        StatKind.Errors => "errors",
        StatKind.Moved => "moved",
        StatKind.WouldMove => "would move",
        StatKind.Renamed => "renamed",
        StatKind.Unchanged => "unchanged",
        StatKind.NoMetadata => "no metadata",
        StatKind.RowsChecked => "rows checked",
        StatKind.DeadRows => "dead rows",
        StatKind.AssetsReset => "assets reset",
        StatKind.SidecarsRemoved => "sidecars removed",
        StatKind.SidecarsToRemove => "sidecars to remove",
        StatKind.UnreferencedFiles => "unreferenced files",
        StatKind.FilesRemoved => "files removed",
        StatKind.FilesToRemove => "files to remove",
        _ => kind.ToString(),
    };

    internal static string ChipLabel(RowState state) => state switch
    {
        RowState.Exported => "Exported",
        RowState.Moved => "Moved",
        RowState.Redated => "Redated",
        RowState.Duplicate => "Duplicate",
        RowState.Metadata => "Metadata only",
        RowState.Unchanged => "Nothing new",
        RowState.Missing => "Missing file",
        RowState.Abandoned => "Orphan sidecar",
        RowState.Unclaimed => "Stray sidecar",
        RowState.Unreferenced => "Unreferenced",
        RowState.Failed => "Failed",
        RowState.Other => "Other",
        _ => state.ToString(),
    };

    internal static string ReasonLabel(CleanReason reason) => reason switch
    {
        CleanReason.Missing => "missing file",
        CleanReason.Abandoned => "orphan sidecar",
        CleanReason.Unclaimed => "stray sidecar",
        CleanReason.Unreferenced => "unreferenced",
        _ => reason.ToString(),
    };

    internal static string OutcomeLabel(CleanOutcome outcome) => outcome switch
    {
        CleanOutcome.Removed => "removed",
        CleanOutcome.Kept => "kept",
        CleanOutcome.WouldRemove => "would remove",
        _ => outcome.ToString(),
    };

    internal static string RunTitleText(RunTitle title)
    {
        switch (title)
        {
            case RunTitle.Redate r:
                return $"redate {Count(r.Count)} assets";
            case RunTitle.Run r:
                var tag = r.Dry ? " (dry run)" : "";
                var blank = r.Arg.Length == 0;
                return r.Tab switch
                {
                    Tab.Consolidate => $"consolidate {r.Template}{tag}",
                    Tab.Clean => r.Option ? $"clean + unreferenced files{tag}" : $"clean{tag}",
                    Tab.Dates => blank ? "scan dates" : $"find {r.Arg}",
                    Tab.OneDrive => blank ? $"onedrive (whole drive){tag}" : $"onedrive {r.Arg}{tag}",
                    Tab.Dropbox => blank ? $"dropbox (everything){tag}" : $"dropbox {r.Arg}{tag}",
                    Tab.ApplePhotos => $"apple-photos {r.Arg}{tag}",
                    Tab.GoogleTakeout => $"google-takeout {r.Arg}{tag}",
                    _ => $"{r.Arg}{tag}",
                };
            default:
                return "";
        }
    }

    internal static (string? Value, string Label, string? Tooltip) FactParts(Fact fact) => fact switch
    {
        Fact.Found f => (Count(f.Total), $"assets in {f.Name}", f.Place),
        Fact.VisibleToPhotoKit f => (Count(f.Total), "visible to PhotoKit", null),
        Fact.Changed f => (Count(f.Total), "new or changed", null),
        Fact.Inventory f => (Count(f.Files), $"files, {Count(f.Sidecars)} sidecars", f.Dir),
        Fact.Warning f => (null, f.Text, null),
        Fact.Listing f => (null, $"listing {f.Name}", f.Path),
        Fact.Reading f => (null, $"reading {f.Name}", f.Path),
        _ => (null, "", null),
    };

    internal static string WithSubject(string? subject, string message) =>
        string.IsNullOrEmpty(subject) ? message : $"{subject}: {message}";

    internal static string KeyLabel(DateKey key) => key switch
    {
        DateKey.Create => "Create",
        DateKey.Original => "Original",
        DateKey.Modify => "Modify",
        _ => key.ToString(),
    };

    internal static string DatesSummary(DatesView view)
    {
        var prefix = view.Scanning && view.Scan is { } scan ? $"checked {Count(scan.Checked)}/{Count(scan.Total)}, " : "";
        return $"{prefix}{Count(view.Rows)} {(view.Query ? "matching" : "flagged")}, {Count(view.Chosen)} chosen";
    }

    internal static string DatesEmpty(DatesView view) =>
        view.Scanning ? "Checking…" : view.Query ? "Nothing matches." : "Every asset's folder date matches its file.";

    internal static string DatesApply(DatesView view) => view.Applying ? "Applying…" : $"Apply {Count(view.Chosen)}";

    internal static string ErrorText(Exception e, Tab? tab = null) => e switch
    {
        CoreException.Blocked b when tab is { } t => BlockerText(t, b.blocker),
        CoreException.Running r => $"{TabName(r.tab)} is running.",
        CoreException.NothingChosen => "Nothing chosen.",
        CoreException.InvalidSettings s => ProblemText(s.problem),
        CoreException.Failed f => f.detail,
        _ => e.Message,
    };

    internal static string ProblemText(SettingsProblem problem) => problem switch
    {
        SettingsProblem.FromDate p => $"from date \"{p.Value}\" is not YYYY-MM-DD",
        SettingsProblem.ToDate p => $"to date \"{p.Value}\" is not YYYY-MM-DD",
        SettingsProblem.Offset p => $"date_zone \"{p.Value}\" is not asset, local, utc or ±HH:MM",
        SettingsProblem.UnknownFields p =>
            $"{(p.Template == TemplateKind.Folder ? "path" : "filename")} uses unknown field{(p.Names.Length == 1 ? "" : "s")} {string.Join(", ", p.Names)}",
        SettingsProblem.FromAfterTo => "filter.from is after filter.to",
        SettingsProblem.CheckInterval => "Check interval must be between 1 and 1440 minutes.",
        _ => "",
    };

    internal static string DateSourceLabel(DateSource source) => source switch
    {
        DateSource.Source => "Source metadata first",
        DateSource.File => "File EXIF first",
        _ => source.ToString(),
    };

    internal static string ZoneLabel(ZoneMode zone) => zone switch
    {
        ZoneMode.Asset => "As recorded",
        ZoneMode.Local => "This computer's zone",
        ZoneMode.Utc => "UTC",
        ZoneMode.Fixed => "Fixed offset",
        _ => zone.ToString(),
    };

    internal static string MediaLabel(Media media) => media switch
    {
        Media.All => "All",
        Media.Photos => "Photos",
        Media.Videos => "Videos",
        _ => media.ToString(),
    };

    internal static string MetadataLabel(Metadata metadata) => metadata switch
    {
        Metadata.Both => "In files + sidecar",
        Metadata.Embed => "In files",
        Metadata.Sidecar => "Sidecar only",
        _ => metadata.ToString(),
    };

    internal static string LayoutSummary(Settings s)
    {
        var path = s.Path.Length == 0 ? "…" : s.Path;
        var file = s.Filename.Length == 0 ? "{stem}" : s.Filename;
        return $"{path}/{file}";
    }

    internal static string ClockSummary(Settings s)
    {
        var source = s.DateSource == DateSource.File ? "file EXIF first" : "source metadata first";
        var zone = s.Zone switch
        {
            ZoneMode.Asset => "as recorded",
            ZoneMode.Local => "this computer's zone",
            ZoneMode.Utc => "UTC",
            _ => "UTC" + (s.Offset.Trim().Length == 0 ? "+00:00" : s.Offset.Trim()),
        };
        return source + Separator + zone;
    }

    internal static bool Filtering(Settings s) => FilterParts(s).Count > 0;

    internal static string FilterSummary(Settings s)
    {
        var parts = FilterParts(s);
        return parts.Count == 0 ? "everything" : string.Join(Separator, parts);
    }

    internal static string FilterHint(Settings s) =>
        Filtering(s) ? "Only matching assets are exported. Consolidate and Dates ignore the filter." : "Everything is exported.";

    internal static string FilesSummary(Settings s)
    {
        var edits = s.Edits ? "originals + edits" : "originals";
        var where = s.Metadata switch
        {
            Metadata.Embed => "in files",
            Metadata.Sidecar => "in sidecar",
            _ => "in files + sidecar",
        };
        return $"{edits}{Separator}metadata {where}";
    }

    internal static string UnknownFields(string[] names) =>
        $"Unknown field{(names.Length == 1 ? "" : "s")}: {string.Join(", ", names)}. Pick from the list; it won't be saved until it's fixed.";

    internal static string PreviewNote(Preview preview) =>
        preview.ExportedStem is { } stem ? $"from {stem}" : "sample photo until something is exported";

    internal static string FieldHelp(string name) => name switch
    {
        "take.year" => "year taken, 2019",
        "take.month" => "month taken, 05",
        "take.day" => "day taken, 03",
        "take.date" => "date taken, 2019-05-03",
        "place.name" => "place named by the source; {place.name|place.city} falls back",
        "place.city" => "city",
        "place.state" => "state or province",
        "place.country" => "country code, FR",
        "keyword" => "keyword n",
        "album" => "album n",
        "label" => "machine label n",
        "people" => "person n",
        "camera.make" => "Apple",
        "camera.model" => "iPhone 15 Pro",
        "camera.lens" => "lens model",
        "title" => "title",
        "type" => "photo or video",
        "stem" => "filename without extension",
        "uuid" => "asset id",
        _ => "",
    };

    private static List<string> FilterParts(Settings s)
    {
        var parts = new List<string>();
        if (s.Media == Media.Photos)
        {
            parts.Add("photos");
        }
        else if (s.Media == Media.Videos)
        {
            parts.Add("videos");
        }
        if (s.Favorites)
        {
            parts.Add("favourites");
        }
        var from = s.From.Trim();
        var to = s.To.Trim();
        if (from.Length > 0 || to.Length > 0)
        {
            parts.Add($"{(from.Length == 0 ? "…" : from)} to {(to.Length == 0 ? "…" : to)}");
        }
        var albums = Joined(s.Albums);
        if (albums.Length > 0)
        {
            parts.Add($"albums: {albums}");
        }
        var keywords = Joined(s.Keywords);
        if (keywords.Length > 0)
        {
            parts.Add($"keywords: {keywords}");
        }
        return parts;
    }

    private static string Joined(string text) =>
        string.Join(", ", text.Split(',').Select(x => x.Trim()).Where(x => x.Length > 0));
}
